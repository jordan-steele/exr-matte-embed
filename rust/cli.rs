use anyhow::{Context, Result};
use clap::Parser;
use exr_matte_embed::{
    batch::{self, BatchEvent, BatchOptions, Plan},
    codec::Codec,
    sequences,
    settings::default_workers,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about = "Batch embed EXR mattes, or scan a delivery folder")]
struct Args {
    /// Folder containing source sequences and their _matte* siblings.
    folder: Option<PathBuf>,
    #[arg(short = 'c', long, value_enum, default_value = "piz")]
    compression: Codec,
    #[arg(short = 'm', long, default_value = "matte")]
    matte_channel: String,
    /// Parallel frame workers. Codec threads are budgeted automatically.
    #[arg(short = 'p', long, alias = "processes", default_value_t = default_workers())]
    workers: usize,
    /// Optional destination root; otherwise write beside sources in *_embedded.
    #[arg(long)]
    output_root: Option<PathBuf>,
    #[arg(short = 'r', long)]
    replace_originals: bool,
    #[arg(short = 's', long)]
    scan_only: bool,
    #[arg(short = 'q', long, conflicts_with = "verbose")]
    quiet: bool,
    #[arg(short = 'v', long)]
    verbose: bool,
    /// Save a JSON scan or processing report to a new file.
    #[arg(long)]
    report: Option<PathBuf>,
    #[arg(long, hide = true)]
    worker: bool,
}

fn save_report(path: &PathBuf, value: &impl serde::Serialize) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), value)?;
    temporary
        .persist_noclobber(path)
        .context("Report already exists or could not be saved")?;
    Ok(())
}

fn run(args: Args) -> Result<i32> {
    if args.worker {
        batch::worker_main()?;
        return Ok(0);
    }
    let folder = args
        .folder
        .context("Supply a delivery folder; use --help for options")?;
    if let Some(path) = &args.report {
        anyhow::ensure!(!path.exists(), "Report already exists: {}", path.display());
    }
    sequences::validate_prefix(&args.matte_channel)?;
    let scan = sequences::scan(&folder)?;
    if !args.quiet {
        println!(
            "{} sequences / {} frames",
            scan.sequences.len(),
            scan.sequences.iter().map(|s| s.files.len()).sum::<usize>()
        );
        for sequence in &scan.sequences {
            let channels = sequence.channel_names(&args.matte_channel);
            println!(
                "  {}  {} frames  {}",
                sequence.name(),
                sequence.files.len(),
                channels
                    .map(|channels| channels.join(", "))
                    .unwrap_or_else(|error| error.to_string())
            );
            if args.verbose {
                println!("    {}", sequence.folder.display());
            }
            for issue in &sequence.issues {
                eprintln!("    Needs attention: {issue}");
            }
        }
    }
    for warning in &scan.warnings {
        eprintln!("Warning: {warning}");
    }
    if args.scan_only {
        if let Some(path) = &args.report {
            save_report(path, &scan)?;
        }
        return Ok(if scan.sequences.iter().any(|s| !s.ready()) {
            1
        } else {
            0
        });
    }
    if scan.sequences.is_empty() {
        anyhow::bail!("No matching numbered EXR sequences found");
    }
    // Headless batches fail preflight on invalid sequences rather than omitting them silently.
    let plan = Plan::new(
        scan.sequences,
        BatchOptions {
            compression: args.compression,
            matte_channel: args.matte_channel,
            workers: args.workers,
            output_root: args.output_root,
            replace_originals: args.replace_originals,
        },
    )?;
    let stop = Arc::new(AtomicBool::new(false));
    let signal_stop = stop.clone();
    ctrlc::set_handler(move || {
        signal_stop.store(true, Ordering::Release);
    })?;
    let (tx, rx) = mpsc::channel();
    let executable = std::env::current_exe()?;
    let handle = std::thread::spawn(move || batch::run(plan, &executable, stop, tx));
    let mut last = Instant::now();
    while let Ok(event) = rx.recv() {
        match event {
            BatchEvent::Progress(progress) => {
                if let Some(error) = &progress.error {
                    eprintln!(
                        "Frame {} in {} failed: {error}",
                        progress.frame,
                        progress.sequence.display()
                    );
                }
                if !args.quiet && (last.elapsed() >= Duration::from_millis(500) || args.verbose) {
                    eprintln!(
                        "{} frames finished / {} embedded / {:.1}s",
                        progress.completed, progress.successful, progress.elapsed_seconds
                    );
                    last = Instant::now();
                }
            }
            BatchEvent::Replacing(folder) => {
                if !args.quiet {
                    eprintln!("Replacing {}", folder.display());
                }
            }
            BatchEvent::Finished(_) => break,
        }
    }
    let report = handle
        .join()
        .map_err(|_| anyhow::anyhow!("Batch worker panicked"))?;
    for error in &report.replacement_errors {
        eprintln!("Replacement: {error}");
    }
    if let Some(path) = &args.report {
        save_report(path, &report)?;
    }
    if !args.quiet {
        println!(
            "{}/{} embedded in {:.3}s; {} errors{}",
            report.successful,
            report.total,
            report.elapsed_seconds,
            report.failures.len() + report.replacement_errors.len(),
            if report.cancelled { "; stopped" } else { "" }
        );
    }
    Ok(if report.cancelled {
        130
    } else if report.success() {
        0
    } else {
        1
    })
}

fn main() {
    let args = Args::parse();
    match run(args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::exit(1);
        }
    }
}
