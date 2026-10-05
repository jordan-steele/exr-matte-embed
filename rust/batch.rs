use crate::{
    MatteInput,
    codec::Codec,
    embed_file,
    sequences::{Sequence, require_unique_destinations},
    settings::cpu_count,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::Sender,
    },
    time::Instant,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOptions {
    pub compression: Codec,
    pub matte_channel: String,
    pub workers: usize,
    pub output_root: Option<PathBuf>,
    pub replace_originals: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameRequest {
    pub base: PathBuf,
    pub mattes: Vec<MatteInput>,
    pub output: PathBuf,
    pub compression: Codec,
}

#[derive(Debug, Serialize, Deserialize)]
struct FrameResponse {
    error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub sequence: PathBuf,
    pub frame: u64,
    pub request: FrameRequest,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub sequences: Vec<Sequence>,
    pub jobs: Vec<Job>,
    pub options: BatchOptions,
}

impl Plan {
    pub fn new(sequences: Vec<Sequence>, options: BatchOptions) -> Result<Self> {
        ensure!(!sequences.is_empty(), "Select at least one valid sequence");
        ensure!(
            (1..=cpu_count()).contains(&options.workers),
            "Parallel frames must be between 1 and {}",
            cpu_count()
        );
        ensure!(
            !options.replace_originals || options.output_root.is_none(),
            "Replacement needs outputs beside the originals; use no custom output root"
        );
        require_unique_destinations(&sequences, options.output_root.as_deref())?;
        let mut jobs = Vec::new();
        for sequence in &sequences {
            ensure!(
                sequence.ready(),
                "{} needs attention: {}",
                sequence.name(),
                sequence.issues.join("; ")
            );
            let channels = sequence.channel_names(&options.matte_channel)?;
            let destination = sequence.output_folder(options.output_root.as_deref());
            let destination_exists = destination.exists();
            ensure!(
                !options.replace_originals || !destination_exists,
                "Replacement needs a new output folder; {} already exists",
                destination.display()
            );
            for (frame, base) in &sequence.files {
                let output = destination.join(base.file_name().context("Missing filename")?);
                ensure!(
                    !destination_exists || !output.exists(),
                    "Output already exists: {}. Choose a new destination",
                    output.display()
                );
                let mattes = sequence
                    .mattes
                    .iter()
                    .zip(&channels)
                    .map(|(matte, channel)| {
                        Ok(MatteInput {
                            channel: channel.clone(),
                            path: matte
                                .files
                                .get(frame)
                                .with_context(|| format!("Missing matte frame {frame}"))?
                                .clone(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                jobs.push(Job {
                    sequence: sequence.folder.clone(),
                    frame: *frame,
                    request: FrameRequest {
                        base: base.clone(),
                        mattes,
                        output,
                        compression: options.compression,
                    },
                });
            }
        }
        Ok(Self {
            sequences,
            jobs,
            options,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameFailure {
    pub sequence: PathBuf,
    pub frame: u64,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub completed: usize,
    pub successful: usize,
    pub sequence: PathBuf,
    pub frame: u64,
    pub elapsed_seconds: f64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchReport {
    pub options: BatchOptions,
    pub sequences: Vec<PathBuf>,
    pub total: usize,
    pub completed: usize,
    pub successful: usize,
    pub failures: Vec<FrameFailure>,
    pub cancelled: bool,
    pub elapsed_seconds: f64,
    pub replaced: Vec<PathBuf>,
    pub replacement_errors: Vec<String>,
}

impl BatchReport {
    pub fn success(&self) -> bool {
        !self.cancelled
            && self.successful == self.total
            && self.failures.is_empty()
            && self.replacement_errors.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum BatchEvent {
    Progress(Progress),
    Replacing(PathBuf),
    Finished(BatchReport),
}

/// Persistent Rust child processes give each codec pool its own thread budget.
/// Command::env avoids mutating the GUI process environment while threads run.
struct Worker {
    child: Child,
    input: Option<BufWriter<ChildStdin>>,
    output: BufReader<ChildStdout>,
}

impl Worker {
    fn start(executable: &Path, codec_threads: usize) -> Result<Self> {
        let mut child = Command::new(executable)
            .arg("--worker")
            .env("RAYON_NUM_THREADS", codec_threads.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Starting Rust frame worker")?;
        let input = BufWriter::new(child.stdin.take().context("Worker input unavailable")?);
        let output = BufReader::new(child.stdout.take().context("Worker output unavailable")?);
        Ok(Self {
            child,
            input: Some(input),
            output,
        })
    }

    fn process(&mut self, request: &FrameRequest) -> Result<()> {
        let input = self.input.as_mut().context("Worker stopped")?;
        serde_json::to_writer(&mut *input, request)?;
        input.write_all(b"\n")?;
        input.flush()?;
        let mut line = String::new();
        ensure!(
            self.output.read_line(&mut line)? > 0,
            "Rust frame worker exited before replying"
        );
        let response: FrameResponse =
            serde_json::from_str(&line).context("Invalid frame worker reply")?;
        if let Some(error) = response.error {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.wait();
    }
}

pub fn worker_main() -> Result<()> {
    // The parent handles Ctrl-C and stops assigning work. Let in-flight EXRs
    // finish atomically even when the terminal signals the whole process group.
    ctrlc::set_handler(|| {}).context("Installing frame worker signal handler")?;
    let mut output = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let response = match serde_json::from_str::<FrameRequest>(&line?) {
            Ok(request) => FrameResponse {
                error: embed_file(
                    &request.base,
                    &request.mattes,
                    &request.output,
                    request.compression.into(),
                )
                .err()
                .map(|error| format!("{error:#}")),
            },
            Err(error) => FrameResponse {
                error: Some(format!("Invalid frame request: {error}")),
            },
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

pub fn run(
    plan: Plan,
    executable: &Path,
    stop: Arc<AtomicBool>,
    events: Sender<BatchEvent>,
) -> BatchReport {
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let successful = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let workers = plan.options.workers.min(plan.jobs.len()).max(1);
    // Budget from the workers actually started so small batches still use every core.
    let codec_threads = (cpu_count() / workers).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                let mut worker = Worker::start(executable, codec_threads);
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = plan.jobs.get(index) else {
                        break;
                    };
                    let outcome = match &mut worker {
                        Ok(worker) => worker.process(&job.request),
                        Err(error) => Err(anyhow::anyhow!("{error:#}")),
                    };
                    let error = outcome.err().map(|error| format!("{error:#}"));
                    if let Some(error) = &error {
                        failures.lock().unwrap().push(FrameFailure {
                            sequence: job.sequence.clone(),
                            frame: job.frame,
                            error: error.clone(),
                        });
                    } else {
                        successful.fetch_add(1, Ordering::Relaxed);
                    }
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = events.send(BatchEvent::Progress(Progress {
                        completed: done,
                        successful: successful.load(Ordering::Relaxed),
                        sequence: job.sequence.clone(),
                        frame: job.frame,
                        elapsed_seconds: started.elapsed().as_secs_f64(),
                        error,
                    }));
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    let mut report = BatchReport {
        options: plan.options.clone(),
        sequences: plan
            .sequences
            .iter()
            .map(|sequence| sequence.folder.clone())
            .collect(),
        total: plan.jobs.len(),
        completed: completed.load(Ordering::Relaxed),
        successful: successful.load(Ordering::Relaxed),
        failures,
        cancelled: stop.load(Ordering::Acquire),
        elapsed_seconds: 0.0,
        replaced: Vec::new(),
        replacement_errors: Vec::new(),
    };
    if plan.options.replace_originals && report.success() {
        for sequence in &plan.sequences {
            if stop.load(Ordering::Acquire) {
                report.cancelled = true;
                break;
            }
            let _ = events.send(BatchEvent::Replacing(sequence.folder.clone()));
            match replace_sequence(sequence, &|path| {
                trash::delete(path).map_err(anyhow::Error::from)
            }) {
                Ok(()) => report.replaced.push(sequence.folder.clone()),
                Err(error) => {
                    report.replacement_errors.push(format!("{error:#}"));
                    break;
                }
            }
        }
    }
    report.elapsed_seconds = started.elapsed().as_secs_f64();
    let _ = events.send(BatchEvent::Finished(report.clone()));
    report
}

/// Publish before trashing. Failed publication restores the original folder;
/// failed trashing leaves a named original in a retained backup directory.
/// Frames are written without a per-frame flush, so every output is synced to
/// disk here before any original moves.
pub fn replace_sequence(
    sequence: &Sequence,
    send_to_trash: &impl Fn(&Path) -> Result<()>,
) -> Result<()> {
    let output = sequence.output_folder(None);
    for file in sequence.files.values() {
        let embedded = output.join(file.file_name().context("Missing filename")?);
        ensure!(embedded.is_file(), "Incomplete replacement output");
        // Write access lets Windows flush the file; nothing is truncated.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&embedded)
            .and_then(|file| file.sync_all())
            .with_context(|| format!("Flushing {} to disk", embedded.display()))?;
    }
    let parent = sequence.folder.parent().context("Source has no parent")?;
    let holding = tempfile::Builder::new()
        .prefix(".exr-original-")
        .tempdir_in(parent)?
        .keep();
    let backup = holding.join(sequence.name());
    if let Err(error) = std::fs::rename(&sequence.folder, &backup) {
        let _ = std::fs::remove_dir(&holding);
        return Err(error).context("Staging originals for replacement");
    }
    if let Err(error) = std::fs::rename(&output, &sequence.folder) {
        std::fs::rename(&backup, &sequence.folder).with_context(|| {
            format!(
                "Publication failed ({error}); originals remain at {}",
                backup.display()
            )
        })?;
        let _ = std::fs::remove_dir(&holding);
        return Err(error).context("Publishing replacement; original folder restored");
    }
    send_to_trash(&backup).with_context(|| {
        format!(
            "Embedded files published. Original backup remains at {}",
            backup.display()
        )
    })?;
    let _ = std::fs::remove_dir(&holding);
    for matte in &sequence.mattes {
        send_to_trash(&matte.folder).with_context(|| {
            format!(
                "Embedded files published. Matte remains at {}",
                matte.folder.display()
            )
        })?;
    }
    Ok(())
}
