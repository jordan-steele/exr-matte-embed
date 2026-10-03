#!/usr/bin/env python3
"""Benchmark the Python 1.1.0 processor against the release Rust embedding core.

Sources are read through symlinks; all outputs go into a new benchmark directory.
Warm cached input, alternate run order, and validate outputs outside the timer.
The original Python multiprocessing code and the Rust embed_file code are used.
"""

import argparse
import collections
import csv
from datetime import datetime
import hashlib
import json
import os
import platform
import queue
import random
import shutil
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO))


def manifest_jobs(path):
    with path.open(newline="") as stream:
        return [(Path(base), Path(matte), Path(output))
                for base, matte, output in csv.reader(stream, delimiter="\t", quoting=csv.QUOTE_NONE)]


def python_baseline():
    for ref in ("python-1.1.0", "origin/python-1.1.0"):
        try:
            commit = subprocess.check_output(["git", "rev-parse", "--verify", f"{ref}^{{commit}}"],
                                             cwd=REPO, stderr=subprocess.DEVNULL, text=True).strip()
        except subprocess.CalledProcessError:
            continue
        source = subprocess.check_output(["git", "show", f"{commit}:src/processing/exr_processor.py"], cwd=REPO)
        return commit, source
    raise ValueError("Fetch the python-1.1.0 archive branch before benchmarking")


def python_worker(manifest, workers, baseline_dir):
    sys.path.insert(0, str(baseline_dir))
    from exr_processor import EXRProcessor

    processor = EXRProcessor()
    grouped = collections.defaultdict(list)
    for base, matte, output in manifest_jobs(manifest):
        if output != Path(str(base.parent) + "_embedded") / base.name:
            raise ValueError("Manifest output does not match the Python processor's output path")
        grouped[(base.parent, matte.parent)].append((base, matte))
    pairs = [{"base_folder": str(base_folder), "matte_folders": {"base": str(matte_folder)},
              "base_files": [base.name for base, _ in frames],
              "matte_files": {"base": [matte.name for _, matte in frames]}, "channels": ["base"]}
             for (base_folder, matte_folder), frames in grouped.items()]
    scans = {"pairs": pairs, "warnings": []}
    progress, results, stop = queue.Queue(), queue.Queue(), threading.Event()
    started = time.perf_counter()
    processor.process_sequences_from_cache(scans, "piz", "matte", workers, progress, results, stop,
                                           replace_originals=False)
    seconds = time.perf_counter() - started
    result = results.get_nowait()
    if result.get("error_files") or not result.get("success"):
        raise RuntimeError(f"Python processor failed: {result}")
    print(json.dumps({"elapsed_seconds": seconds, "completed": sum(len(p["base_files"]) for p in pairs),
                      "workers": workers}), flush=True)


def write_rgba_fixture(base, matte, destination):
    """Add a varying alpha from the matte to a new lossless benchmark input."""
    import Imath
    import OpenEXR
    from validate_exr_backend import half_red, read_header

    source = OpenEXR.InputFile(str(base))
    try:
        header = source.header()
        if "A" in header["channels"]:
            raise ValueError(f"Input already has alpha; omit --add-alpha-from-matte: {base}")
        pixels = {name: source.channel(name, channel.type) for name, channel in header["channels"].items()}
    finally:
        source.close()
    header["channels"]["A"] = Imath.Channel(Imath.PixelType(Imath.PixelType.HALF))
    pixels["A"] = half_red(matte)
    header["compression"] = Imath.Compression(Imath.Compression.PIZ_COMPRESSION)
    header.pop("writer", None)
    output = OpenEXR.OutputFile(str(destination), header)
    try:
        output.writePixels(pixels)
    finally:
        output.close()
    fixture = OpenEXR.InputFile(str(destination))
    try:
        for name, expected in pixels.items():
            if fixture.channel(name, header["channels"][name].type) != expected:
                raise AssertionError(f"Fixture changed {name} pixels: {destination}")
    finally:
        fixture.close()
    before, after = read_header(base), read_header(destination)
    if any(after.get(name) != value for name, value in before.items()
           if name not in {"channels", "compression", "writer"}):
        raise AssertionError(f"Fixture changed source metadata: {destination}")


def prepare_jobs(source_root, output_root, frames_per_sequence, sequence_name=None, add_alpha=False):
    from validate_exr_backend import frames

    jobs, sequences = [], []
    for index, matte_folder in enumerate(sorted(source_root.glob("*_matte"))):
        base_folder = matte_folder.with_name(matte_folder.name.removesuffix("_matte"))
        if sequence_name is not None and base_folder.name != sequence_name:
            continue
        base_frames, matte_frames = frames(base_folder), frames(matte_folder)
        if base_frames.keys() != matte_frames.keys():
            raise ValueError(f"Frame numbers differ: {base_folder}")
        numbers = sorted(base_frames)
        count = min(frames_per_sequence, len(numbers))
        if not count:
            raise ValueError(f"Empty sequence: {base_folder}")
        selected = [numbers[round(i * (len(numbers) - 1) / max(count - 1, 1))] for i in range(count)]
        alias = output_root / "input-links" / f"sequence-{index:02d}"
        base_alias, matte_alias = alias / "base", alias / "matte"
        base_alias.mkdir(parents=True)
        matte_alias.mkdir()
        for number in selected:
            base_link, matte_link = base_alias / base_frames[number].name, matte_alias / matte_frames[number].name
            if add_alpha:
                write_rgba_fixture(base_frames[number], matte_frames[number], base_link)
            else:
                base_link.symlink_to(base_frames[number])
            matte_link.symlink_to(matte_frames[number])
            output = Path(str(base_alias) + "_embedded") / base_link.name
            jobs.append((base_link, matte_link, output))
        sequences.append({"folder": base_folder.name, "available_frames": len(numbers), "selected_frames": selected})
    if not jobs:
        raise ValueError("No matched base/matte sequences")
    return jobs, sequences


def write_manifest(path, jobs):
    with path.open("w", newline="") as stream:
        writer = csv.writer(stream, delimiter="\t", lineterminator="\n", quoting=csv.QUOTE_NONE)
        for job in jobs:
            if any("\t" in str(field) or "\n" in str(field) for field in job):
                raise ValueError("Benchmark paths cannot contain tabs or newlines")
            writer.writerow(map(str, job))


def clean_outputs(jobs):
    # Every directory here is generated underneath this run's new output root.
    for directory in {output.parent for _, _, output in jobs}:
        if directory.exists():
            shutil.rmtree(directory)


def warm_sources(jobs):
    for path in {path.resolve() for base, matte, _ in jobs for path in (base, matte)}:
        with path.open("rb") as stream:
            while stream.read(4 * 1024 * 1024):
                pass


def validate_outputs(jobs):
    import Imath
    import OpenEXR
    from validate_exr_backend import channel_descriptions, half_red, read_header

    sizes, writer_removed = [], False
    for base, matte, output in jobs:
        before, after = read_header(base), read_header(output)
        changed = [name for name, value in before.items()
                   if name not in {"channels", "compression", "writer"} and after.get(name) != value]
        if changed:
            raise AssertionError(f"Changed metadata in {output}: {changed}")
        writer_removed |= "writer" in before and "writer" not in after
        channels, out_channels = channel_descriptions(before), channel_descriptions(after)
        if set(out_channels) != set(channels) | {"matte"} or out_channels["matte"]["type"] != 1:
            raise AssertionError(f"Unexpected output channels: {output}")
        if after["compression"][1] != bytes([4]):
            raise AssertionError(f"Output is not PIZ: {output}")
        original, embedded = OpenEXR.InputFile(str(base)), OpenEXR.InputFile(str(output))
        try:
            for name, description in channels.items():
                if out_channels[name] != description:
                    raise AssertionError(f"Changed channel description: {name}")
                pixel_type = Imath.PixelType(description["type"])
                if original.channel(name, pixel_type) != embedded.channel(name, pixel_type):
                    raise AssertionError(f"Changed {name} pixels: {output}")
            if embedded.channel("matte", Imath.PixelType(Imath.PixelType.HALF)) != half_red(matte):
                raise AssertionError(f"Changed matte pixels: {output}")
        finally:
            original.close()
            embedded.close()
        sizes.append(output.stat().st_size)
    return {"frames_checked": len(jobs), "original_pixels": "exact", "matte_pixels": "exact",
            "source_metadata_except_writer": "exact", "writer_removed": writer_removed,
            "output_bytes": sum(sizes)}


def summarize(report):
    for config in report["configurations"]:
        runs = report["runs"].get(config["name"], [])
        if not runs:
            continue
        values = [run["elapsed_seconds"] for run in runs]
        median = statistics.median(values)
        config["summary"] = {"median_seconds": median, "min_seconds": min(values), "max_seconds": max(values),
                             "frames_per_second": report["frame_count"] / median,
                             "mean_ms_per_frame": 1000 * median / report["frame_count"], "trials": len(values)}
    if report.get("complete"):
        measured = [config for config in report["configurations"] if "summary" in config]
        python = [config for config in measured if config["engine"] == "python"]
        rust = [config for config in measured if config["engine"] == "rust"]
        if python and rust:
            fastest_python = min(python, key=lambda config: config["summary"]["median_seconds"])
            fastest_rust = min(rust, key=lambda config: config["summary"]["median_seconds"])
            speedup = fastest_python["summary"]["median_seconds"] / fastest_rust["summary"]["median_seconds"]
            report["comparison"] = {"fastest_python": fastest_python["name"],
                                    "fastest_rust": fastest_rust["name"],
                                    "rust_speedup_vs_fastest_python": speedup,
                                    "rust_same_or_faster_by_median": speedup >= 1.0}


def write_report(report, output_root):
    summarize(report)
    (output_root / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    lines = ["# Python / Rust EXR benchmark", "", f"Frames per trial: {report['frame_count']}; PIZ compression.",
             "Input file cache warmed before the sweep; common output filesystem; release Rust build.", "",
             "| Configuration | Median seconds | Frames/s | Min–max seconds | Trials |",
             "|---|---:|---:|---:|---:|"]
    for config in report["configurations"]:
        if summary := config.get("summary"):
            lines.append(f"| {config['name']} | {summary['median_seconds']:.3f} | {summary['frames_per_second']:.2f} | "
                         f"{summary['min_seconds']:.3f}–{summary['max_seconds']:.3f} | {summary['trials']} |")
    if comparison := report.get("comparison"):
        status = "PASS" if comparison["rust_same_or_faster_by_median"] else "FAIL"
        lines.extend(["", f"Median performance check: **{status}**. Fastest Rust is "
                      f"{comparison['rust_speedup_vs_fastest_python']:.2f}× as fast as fastest Python."])
    checked = sum(result["frames_checked"] for result in report.get("validation", {}).values())
    if report.get("input_variant") == "generated-rgba":
        lines.extend(["", "Inputs are generated RGBA fixtures: original base channels plus HALF alpha from the matte's R channel.",
                      "Fixture generation is excluded from timing; these are not original RGBA delivery files."])
    lines.extend(["", f"Independently checked outputs: {checked} (first trial of each configuration).",
                  "Timing includes file reads, channel embedding, compression, writes, and worker/pool creation.",
                  "Interpreter startup, source discovery, build time, cache warming, validation, and cleanup are excluded.",
                  "Python uses the original 1.1.0 multiprocessing processor. Rust calls the existing embedding core.",
                  "Rust retains its atomic output publication and fsync; the original Python processor does neither.",
                  "Sources remain read-only; generated outputs are deleted between trials.",
                  "These are processing measurements with prewarmed inputs; cache eviction is not controlled.",
                  "They do not measure cold-drive reads or the full UI.", ""])
    (output_root / "report.md").write_text("\n".join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, nargs="?")
    parser.add_argument("--output-root", type=Path)
    parser.add_argument("--rust-binary", type=Path, default=REPO / "target/release/exr-matte-benchmark")
    parser.add_argument("--frames-per-sequence", type=int, default=8)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--sequence-name", help="Benchmark only this base sequence folder")
    parser.add_argument("--add-alpha-from-matte", action="store_true",
                        help="Create lossless input fixtures with HALF alpha from matte R; original files remain unchanged")
    parser.add_argument("--profiles", nargs="+", choices=["python-single", "rust-single", "python-default",
                        "python-full", "rust-current", "rust-four", "rust-half", "rust-full"])
    parser.add_argument("--verify-frames", type=int, default=0,
                        help="Check this many evenly spaced outputs per profile; 0 checks every frame")
    parser.add_argument("--worker", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--manifest", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--workers", type=int, help=argparse.SUPPRESS)
    parser.add_argument("--python-baseline-dir", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker:
        if args.manifest is None or args.workers is None or args.python_baseline_dir is None:
            parser.error("Worker requires a manifest, worker count, and archived Python baseline")
        python_worker(args.manifest, args.workers, args.python_baseline_dir)
        return
    if args.root is None or args.output_root is None or args.frames_per_sequence < 1 or args.repeats < 1 or args.verify_frames < 0:
        parser.error("Provide a source root, new --output-root, and positive frame/repeat counts")
    root, output_root, binary = args.root.resolve(), args.output_root.resolve(), args.rust_binary.resolve()
    if output_root == root or root in output_root.parents:
        raise ValueError("Benchmark outputs must be outside the source directory")
    if not binary.is_file():
        raise ValueError("Build the release Rust benchmark binary first")
    # Check dependencies before creating any files or starting a timed trial.
    import OpenEXR
    import numpy  # noqa: F401
    import send2trash  # noqa: F401

    baseline_commit, baseline_source = python_baseline()

    output_root.mkdir(parents=True, exist_ok=False)
    baseline_dir = output_root / "python-baseline"
    baseline_dir.mkdir()
    (baseline_dir / "exr_processor.py").write_bytes(baseline_source)
    jobs, sequences = prepare_jobs(root, output_root, args.frames_per_sequence, args.sequence_name,
                                  args.add_alpha_from_matte)
    manifest = output_root / "jobs.tsv"
    write_manifest(manifest, jobs)
    cpu_count = os.cpu_count() or 1
    default_workers = max(cpu_count // 2, 1)
    configs = [
        {"id": "python-single", "name": "Python: 1 process", "engine": "python", "workers": 1},
        {"id": "rust-single", "name": "Rust: 1 frame / 1 codec thread", "engine": "rust", "workers": 1, "codec_threads": 1},
        {"id": "python-default", "name": f"Python: default {default_workers} processes", "engine": "python", "workers": default_workers},
        {"id": "python-full", "name": f"Python: {cpu_count} processes", "engine": "python", "workers": cpu_count},
        {"id": "rust-current", "name": f"Rust: current 1 frame / {cpu_count} codec threads", "engine": "rust", "workers": 1, "codec_threads": cpu_count},
        {"id": "rust-four", "name": f"Rust: {min(4, cpu_count)} frames / {max(cpu_count // min(4, cpu_count), 1)} codec threads each", "engine": "rust", "workers": min(4, cpu_count), "codec_threads": max(cpu_count // min(4, cpu_count), 1)},
        {"id": "rust-half", "name": f"Rust: {default_workers} frames / 2 codec threads each", "engine": "rust", "workers": default_workers, "codec_threads": 2},
        {"id": "rust-full", "name": f"Rust: {cpu_count} frames / 1 codec thread each", "engine": "rust", "workers": cpu_count, "codec_threads": 1},
    ]
    if args.profiles:
        configs = [config for config in configs if config["id"] in args.profiles]
    verify_count = min(args.verify_frames or len(jobs), len(jobs))
    verify_jobs = [jobs[round(i * (len(jobs) - 1) / max(verify_count - 1, 1))] for i in range(verify_count)]
    report = {"date": datetime.now().astimezone().date().isoformat(), "python_version": platform.python_version(),
              "OpenEXR_version": OpenEXR.__version__, "exr_version": "1.74.2",
              "rust_version": subprocess.check_output(["rustc", "--version"], text=True).strip(),
              "machine": platform.machine(), "platform": platform.platform(), "logical_cpus": cpu_count,
              "source_root": str(root), "output_root": str(output_root), "python_executable": sys.executable,
              "python_source_commit": baseline_commit,
              "python_processor_sha256": hashlib.sha256(baseline_source).hexdigest(),
              "python_baseline_mode": "archive-snapshot",
              "source_bytes": sum(path.stat().st_size for base, matte, _ in jobs for path in (base, matte)),
              "frame_count": len(jobs), "sequences": sequences, "compression": "piz", "input_cache": "warm",
              "input_variant": "generated-rgba" if args.add_alpha_from_matte else "original-deliveries",
              "verification_frames_per_profile": verify_count,
              "seed": 20261002, "repeats": args.repeats, "configurations": configs, "runs": {}, "validation": {}}
    print(f"Benchmark: {len(jobs)} frames, {args.repeats} repeats, {cpu_count} logical CPUs", flush=True)
    warm_sources(jobs)
    # Each timed run is a fresh persistent worker; pool creation remains timed.
    rng = random.Random(report["seed"])
    write_report(report, output_root)
    try:
        for repeat in range(args.repeats):
            order = list(configs)
            rng.shuffle(order)
            for config in order:
                clean_outputs(jobs)
                env = dict(os.environ)
                if config["engine"] == "python":
                    command = [sys.executable, str(Path(__file__).resolve()), "--worker", "--manifest", str(manifest),
                               "--workers", str(config["workers"]), "--python-baseline-dir", str(baseline_dir)]
                else:
                    env["RAYON_NUM_THREADS"] = str(config["codec_threads"])
                    command = [str(binary), "--manifest", str(manifest), "--workers", str(config["workers"])]
                print(f"Trial {repeat + 1}/{args.repeats}: {config['name']}", flush=True)
                started = time.perf_counter()
                result = subprocess.run(command, env=env, capture_output=True, text=True)
                if result.returncode:
                    raise RuntimeError(f"{config['name']} failed:\n{result.stderr}\n{result.stdout}")
                end_to_end = time.perf_counter() - started
                measurement = json.loads(result.stdout.strip().splitlines()[-1])
                if measurement["completed"] != len(jobs) or any(not output.is_file() for _, _, output in jobs):
                    raise AssertionError("A benchmark run did not produce all requested frames")
                measurement["end_to_end_seconds"] = end_to_end
                measurement["stderr"] = result.stderr
                report["runs"].setdefault(config["name"], []).append(measurement)
                print(f"  {measurement['elapsed_seconds']:.3f}s ({len(jobs) / measurement['elapsed_seconds']:.2f} frames/s)", flush=True)
                if repeat == 0:
                    report["validation"][config["name"]] = validate_outputs(verify_jobs)
                    print(f"  Verified {verify_count} outputs: exact source pixels, mattes, and delivery metadata", flush=True)
                write_report(report, output_root)
    finally:
        clean_outputs(jobs)
    report["complete"] = True
    write_report(report, output_root)
    print(f"Completed report: {output_root / 'report.md'}", flush=True)


if __name__ == "__main__":
    main()
