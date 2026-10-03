use std::{path::PathBuf, time::Instant};

use anyhow::{Result, bail};
use clap::Parser;
use exr_matte_embed::{MatteInput, codec::Codec, embed_file};

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
