use std::{path::PathBuf, time::Instant};

use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};
use exr::prelude::Compression;
use exr_matte_embed::{MatteInput, embed_file};

#[derive(Parser)]
#[command(about = "Validate the Rust EXR backend on one delivery frame")]
struct Args {
    #[arg(long)]
    base: PathBuf,
    /// Output channel and source file, e.g. matte.hero=/path/to/hero.exr.
    #[arg(long, required = true, value_parser = parse_matte)]
    matte: Vec<MatteInput>,
    /// A new file; existing files are never overwritten.
    #[arg(long)]
    output: PathBuf,
    #[arg(long, value_enum, default_value = "piz")]
    compression: Codec,
}

#[derive(Clone, Copy, ValueEnum)]
enum Codec {
    None,
    Rle,
    Zip,
    Zips,
    Piz,
    Pxr24,
    B44,
    B44a,
    Dwaa,
    Dwab,
}

impl From<Codec> for Compression {
    fn from(codec: Codec) -> Self {
        match codec {
            Codec::None => Self::Uncompressed,
            Codec::Rle => Self::RLE,
            Codec::Zip => Self::ZIP16,
            Codec::Zips => Self::ZIP1,
            Codec::Piz => Self::PIZ,
            Codec::Pxr24 => Self::PXR24,
            Codec::B44 => Self::B44,
            Codec::B44a => Self::B44A,
            Codec::Dwaa => Self::DWAA(None),
            Codec::Dwab => Self::DWAB(None),
        }
    }
}

fn parse_matte(value: &str) -> Result<MatteInput> {
    let Some((channel, path)) = value.split_once('=') else {
        bail!("Use CHANNEL=FILE for each --matte");
    };
    if path.is_empty() {
        bail!("A matte file path is required");
    }
    Ok(MatteInput {
        channel: channel.to_owned(),
        path: path.into(),
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    let started = Instant::now();
    embed_file(
        &args.base,
        &args.matte,
        &args.output,
        args.compression.into(),
    )?;
    println!(
        "Wrote {} in {:.3}s",
        args.output.display(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
