#!/usr/bin/env python3
"""Check Rust output with the independent OpenEXR reference implementation.

Reads delivery sources without modifying them. Writes samples and a JSON report
under the requested output directory. Requires OpenEXR and NumPy.
"""

import argparse
import collections
import json
import re
import struct
import subprocess
import time
from pathlib import Path

import Imath
import numpy as np
import OpenEXR

CODECS = ["none", "rle", "zips", "zip", "piz", "pxr24", "b44", "b44a", "dwaa", "dwab"]


def read_string(stream):
    value = bytearray()
    while True:
        byte = stream.read(1)
        if not byte:
            raise ValueError("Truncated EXR header")
        if byte == b"\0":
            return value.decode("latin1")
        value.extend(byte)


def read_header(path):
    """Read raw attribute bytes so timecode/rational metadata is checked exactly."""
    attributes = {}
    with path.open("rb") as stream:
        prefix = stream.read(8)
        if len(prefix) != 8:
            raise ValueError(f"Incomplete EXR header: {path}")
        magic, version = struct.unpack("<II", prefix)
        if magic != 20000630 or version & ~0x400 != 2:
            raise ValueError(f"Expected a single-part scanline EXR: {path}")
        while name := read_string(stream):
            kind = read_string(stream)
            size = struct.unpack("<I", stream.read(4))[0]
            if size > 1024 * 1024:
                raise ValueError(f"Unexpectedly large header attribute: {name}")
            value = stream.read(size)
            if len(value) != size:
                raise ValueError("Truncated EXR attribute")
            attributes[name] = (kind, value)
    return attributes


def channel_descriptions(attributes):
    data = attributes["channels"][1]
    position = 0
    descriptions = {}
    while data[position] != 0:
        end = data.index(0, position)
        name = data[position:end].decode("latin1")
        position = end + 1
        sample_type, linear, x, y = struct.unpack_from("<iB3xii", data, position)
        position += 16
        descriptions[name] = {"type": sample_type, "pLinear": linear, "sampling": [x, y]}
    return descriptions


def inventory(root):
    result = []
    for folder in sorted(path for path in root.iterdir() if path.is_dir()):
        files = sorted(path for path in folder.iterdir() if path.suffix.lower() == ".exr")
        signatures = collections.Counter()
        invalid = []
        for path in files:
            try:
                header = read_header(path)
            except (ValueError, struct.error) as error:
                invalid.append({"file": path.name, "bytes": path.stat().st_size, "error": str(error)})
                continue
            signature = json.dumps({
                "compression": CODECS[header["compression"][1][0]],
                "channels": channel_descriptions(header),
                "dataWindow": struct.unpack("<iiii", header["dataWindow"][1]),
                "displayWindow": struct.unpack("<iiii", header["displayWindow"][1]),
            }, sort_keys=True)
            signatures[signature] += 1
        result.append({"folder": folder.name, "count": len(files), "invalid": invalid, "formats": [
            {"count": count, **json.loads(signature)} for signature, count in signatures.items()
        ]})
    return result


def frames(folder):
    indexed = {}
    for path in folder.iterdir():
        if path.suffix.lower() != ".exr":
            continue
        match = re.search(r"\.(\d+)\.exr$", path.name, re.IGNORECASE)
        if not match:
            raise ValueError(f"No frame number: {path}")
        number = int(match[1])
        if number in indexed:
            raise ValueError(f"Duplicate frame {number}: {folder}")
        indexed[number] = path
    return indexed


def run_embed(binary, base, mattes, output, codec="piz", expect_success=True):
    command = [str(binary), "--base", str(base), "--output", str(output), "--compression", codec]
    for channel, path in mattes.items():
        command.extend(["--matte", f"{channel}={path}"])
    started = time.monotonic()
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    elapsed = time.monotonic() - started
    if expect_success:
        if result.returncode:
            raise RuntimeError(result.stderr)
    elif not result.returncode:
        raise AssertionError(f"An invalid operation succeeded: {command}")
    return elapsed


def half_red(path):
    file = OpenEXR.InputFile(str(path))
    try:
        return file.channel("R", Imath.PixelType(Imath.PixelType.HALF))
    finally:
        file.close()


def native_red(path):
    """The matte's R description and samples at its own type; embedding must not convert it."""
    description = channel_descriptions(read_header(path))["R"]
    file = OpenEXR.InputFile(str(path))
    try:
        return description, file.channel("R", Imath.PixelType(description["type"]))
    finally:
        file.close()


def verify(base_path, mattes, output_path, lossy=False):
    base_header, output_header = read_header(base_path), read_header(output_path)
    changed = [name for name, value in base_header.items()
               if name not in {"channels", "compression"} and output_header.get(name) != value]
    if changed:
        raise AssertionError(f"Changed metadata: {changed}")
    base_channels = channel_descriptions(base_header)
    output_channels = channel_descriptions(output_header)
    if set(output_channels) != set(base_channels) | set(mattes):
        raise AssertionError(f"Unexpected output channels: {output_channels}")
    file, output = OpenEXR.InputFile(str(base_path)), OpenEXR.InputFile(str(output_path))
    statistics = {}
    try:
        for name, description in base_channels.items():
            if name in mattes:
                continue
            if output_channels[name] != description:
                raise AssertionError(f"Changed channel description: {name}")
            native_type = Imath.PixelType(description["type"])
            before, after = file.channel(name, native_type), output.channel(name, native_type)
            if not lossy and before != after:
                raise AssertionError(f"Changed pixels in {name}: {base_path}")
            if lossy:
                dtype = {0: np.uint32, 1: np.float16, 2: np.float32}[description["type"]]
                delta = np.frombuffer(before, dtype=dtype).astype(np.float64) - np.frombuffer(after, dtype=dtype)
                if not np.all(np.isfinite(delta)):
                    raise AssertionError(f"Nonfinite output in {name}")
                statistics[name] = {"bit_exact": before == after, "samples": int(delta.size),
                                    "changed_samples": int(np.count_nonzero(delta)),
                                    "max_abs_error": float(np.max(np.abs(delta))),
                                    "rmse": float(np.sqrt(np.mean(delta * delta)))}
        for name, path in mattes.items():
            description, samples = native_red(path)
            if output_channels[name] != description:
                raise AssertionError(f"Matte {name} must keep its source R type and pLinear: "
                                     f"{output_channels[name]} != {description}")
            if output.channel(name, Imath.PixelType(description["type"])) != samples:
                raise AssertionError(f"Matte pixels differ: {name}")
    finally:
        file.close()
        output.close()
    return {"metadata": "exact", "base_pixels": statistics if lossy else "exact",
            "matte_pixels": "exact", "channels": list(output_channels)}


def write_reference(path, channels, codec=Imath.Compression.PIZ_COMPRESSION, origin=(0, 0), custom=False):
    shape = next(iter(channels.values())).shape
    height, width = shape
    header = OpenEXR.Header(width, height)
    header["dataWindow"] = Imath.Box2i(Imath.V2i(*origin), Imath.V2i(origin[0] + width - 1, origin[1] + height - 1))
    header["compression"] = Imath.Compression(codec)
    types = {np.dtype("uint32"): 0, np.dtype("float16"): 1, np.dtype("float32"): 2}
    header["channels"] = {name: Imath.Channel(Imath.PixelType(types[array.dtype])) for name, array in channels.items()}
    if custom:
        header["comments"] = b"Reference mixed-type cropped fixture"
        header["deliveryTag"] = b"matte-validation"
    file = OpenEXR.OutputFile(str(path), header)
    try:
        file.writePixels({name: array.tobytes() for name, array in channels.items()})
    finally:
        file.close()


def synthetic_checks(binary, directory):
    directory.mkdir()
    values = np.arange(28).reshape((4, 7))
    base, matte = directory / "base.exr", directory / "matte.exr"
    write_reference(base, {"R": (values * 1.234567 - 5).astype(np.float32),
                           "G": (values / 28).astype(np.float16),
                           "B": (values + 2**28).astype(np.uint32),
                           "A": np.ones((4, 7), dtype=np.float16),
                           "matte.keep": np.full((4, 7), 0.25, dtype=np.float16),
                           "matte": np.zeros((4, 7), dtype=np.float16)}, origin=(-2, -1), custom=True)
    write_reference(matte, {"R": (values / 28).astype(np.float32)}, origin=(-2, -1))
    output = directory / "mixed-types.exr"
    mattes = {"matte": matte, "matte.hero": matte}
    run_embed(binary, base, mattes, output)
    result = {"mixed_types_and_offset_window": verify(base, mattes, output)}
    for codec in ("none", "rle", "zip", "zips"):
        codec_output = directory / f"mixed-types-{codec}.exr"
        run_embed(binary, base, mattes, codec_output, codec)
        result[codec] = verify(base, mattes, codec_output)
        header = read_header(codec_output)
        if CODECS[header["compression"][1][0]] != codec:
            raise AssertionError(f"Incorrect codec mapping: {codec}")
    original_bytes = output.read_bytes()
    run_embed(binary, base, mattes, output, expect_success=False)
    if output.read_bytes() != original_bytes:
        raise AssertionError("An existing output was modified")
    result["existing_output_protected"] = True
    mismatch = directory / "mismatch.exr"
    write_reference(mismatch, {"R": values.astype(np.float16)}, origin=(-1, -1))
    invalid_output = directory / "invalid.exr"
    run_embed(binary, base, {"matte": mismatch}, invalid_output, expect_success=False)
    if invalid_output.exists():
        raise AssertionError("Mismatched data windows produced output")
    result["window_mismatch_rejected"] = True
    missing_red = directory / "missing-red.exr"
    write_reference(missing_red, {"G": values.astype(np.float16)}, origin=(-2, -1))
    run_embed(binary, base, {"matte": missing_red}, invalid_output, expect_success=False)
    if invalid_output.exists():
        raise AssertionError("A matte without R produced output")
    result["missing_red_rejected"] = True
    run_embed(binary, base, {"R": matte}, invalid_output, expect_success=False)
    if invalid_output.exists():
        raise AssertionError("A matte replaced an original R channel")
    result["rgba_replacement_rejected"] = True
    if any(path.name.startswith(".tmp") for path in directory.iterdir()):
        raise AssertionError("Temporary output was left behind")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--binary", type=Path, default=Path("target/release/exr-matte-prototype"))
    parser.add_argument("--output-root", type=Path, required=True, help="A new directory for validation outputs")
    args = parser.parse_args()
    root, binary, output_root = args.root.resolve(), args.binary.resolve(), args.output_root.resolve()
    if output_root == root or root in output_root.parents:
        raise ValueError("Validation outputs must be outside the source folder")
    output_root.mkdir(parents=True, exist_ok=False)
    report = {"OpenEXR_version": OpenEXR.__version__, "inventory": inventory(root), "piz_samples": []}
    report_path = output_root / "report.json"
    def save_report():
        report_path.write_text(json.dumps(report, indent=2) + "\n")
    save_report()
    first_pair = None
    for matte_folder in sorted(root.glob("*_matte")):
        base_folder = matte_folder.with_name(matte_folder.name.removesuffix("_matte"))
        if not base_folder.is_dir():
            raise ValueError(f"Missing base directory: {matte_folder}")
        base_frames, matte_frames = frames(base_folder), frames(matte_folder)
        if base_frames.keys() != matte_frames.keys():
            raise ValueError(f"Frame mismatch: {base_folder}")
        numbers = sorted(base_frames)
        if not numbers:
            raise ValueError(f"Empty sequence: {base_folder}")
        for number in sorted({numbers[0], numbers[len(numbers) // 2], numbers[-1]}):
            base, matte = base_frames[number], matte_frames[number]
            output = output_root / base_folder.name / base.name
            seconds = run_embed(binary, base, {"matte": matte}, output)
            result = {"sequence": base_folder.name, "frame": number, "seconds": seconds,
                      **verify(base, {"matte": matte}, output)}
            report["piz_samples"].append(result)
            save_report()
            print(f"PASS PIZ {base_folder.name} frame {number}: exact pixels and metadata ({seconds:.3f}s)", flush=True)
            if first_pair is None:
                first_pair = (base, matte)
    if first_pair is None:
        raise ValueError("No matched delivery sequences")
    base, matte = first_pair
    dwa_output = output_root / "rust-dwaa.exr"
    seconds = run_embed(binary, base, {"matte": matte}, dwa_output, "dwaa")
    report["dwaa_write"] = {"seconds": seconds, **verify(base, {"matte": matte}, dwa_output, lossy=True)}
    save_report()
    print("PASS Rust DWAA write: reference decoder reads it; matte pixels and metadata exact", flush=True)
    source = OpenEXR.InputFile(str(base))
    try:
        header = source.header()
        height = header["dataWindow"].max.y - header["dataWindow"].min.y + 1
        width = header["dataWindow"].max.x - header["dataWindow"].min.x + 1
        channels = {name: np.frombuffer(source.channel(name, description.type),
                    dtype={0: np.uint32, 1: np.float16, 2: np.float32}[description.type.v]).reshape((height, width))
                    for name, description in header["channels"].items()}
        reference_dwaa = output_root / "reference-dwaa.exr"
        write_reference(reference_dwaa, channels, Imath.Compression.DWAA_COMPRESSION)
    finally:
        source.close()
    decoded_output = output_root / "reference-dwaa-to-piz.exr"
    seconds = run_embed(binary, reference_dwaa, {"matte": matte}, decoded_output)
    # This is a diagnostic, not an assertion of bit-identical DWA decoders.
    # Keep discrepancies visible in the report instead of hiding them in a tolerance.
    comparison = verify(reference_dwaa, {"matte": matte}, decoded_output, lossy=True)
    exact = all(stats["bit_exact"] for stats in comparison["base_pixels"].values())
    report["dwaa_read"] = {"seconds": seconds, "reference_decoder_bit_exact": exact, **comparison}
    save_report()
    changed = sum(stats["changed_samples"] for stats in comparison["base_pixels"].values())
    print(f"DWAA read diagnostic: {changed} RGB sample differences against reference decoder", flush=True)
    report["synthetic_checks"] = synthetic_checks(binary, output_root / "synthetic")
    print("PASS mixed sample types, offset windows, metadata, and output protection", flush=True)
    save_report()
    print(f"Validation report: {report_path}", flush=True)


if __name__ == "__main__":
    main()
