//! A persistent batch driver for benchmarking the unchanged embedding core.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use exr::prelude::Compression;
use exr_matte_embed::{MatteInput, embed_file};

#[derive(Parser)]
#[command(about = "Benchmark a batch of PIZ frames using the existing Rust core")]
struct Args {
    /// UTF-8 TSV rows: base EXR, matte EXR, output EXR. Paths cannot contain tabs.
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long, default_value_t = 1)]
    workers: usize,
}

struct Job {
    base: PathBuf,
    matte: MatteInput,
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(args.workers > 0, "Workers must be greater than zero");
    let jobs: Vec<Job> = std::fs::read_to_string(&args.manifest)?
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let fields: Vec<_> = line.split('\t').collect();
            ensure!(fields.len() == 3, "Invalid manifest row {}", index + 1);
            Ok(Job {
                base: fields[0].into(),
                matte: MatteInput {
                    channel: "matte".into(),
                    path: fields[1].into(),
                },
                output: fields[2].into(),
            })
        })
        .collect::<Result<_>>()?;
    ensure!(!jobs.is_empty(), "Manifest is empty");
    let next = AtomicUsize::new(0);
    let started = Instant::now();
    std::thread::scope(|scope| -> Result<()> {
        let handles: Vec<_> = (0..args.workers.min(jobs.len()))
            .map(|_| {
                scope.spawn(|| -> Result<()> {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(index) else {
                            return Ok(());
                        };
                        embed_file(
                            &job.base,
                            std::slice::from_ref(&job.matte),
                            &job.output,
                            Compression::PIZ,
                        )
                        .with_context(|| format!("Benchmark frame: {}", job.base.display()))?;
                    }
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("Benchmark worker panicked"))??;
        }
        Ok(())
    })?;
    println!(
        "{{\"elapsed_seconds\":{:.9},\"completed\":{},\"workers\":{}}}",
        started.elapsed().as_secs_f64(),
        jobs.len(),
        args.workers,
    );
    Ok(())
}
