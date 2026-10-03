use crate::Args;
use eframe::egui::{self, Color32, Frame, Margin, RichText, Stroke, TextureHandle, Ui};
use egui_extras::{Column, TableBuilder};
use exr_matte_embed::{
    batch::{self, BatchEvent, BatchOptions, BatchReport, Plan},
    codec::Codec,
    sequences::{self, ScanResult},
    settings::{self, Settings},
};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

enum UiEvent {
    Scan(Result<ScanResult, String>),
    Batch(BatchEvent),
    Failed(String),
}

#[derive(Default)]
struct SequenceProgress {
    completed: usize,
    failed: usize,
}

#[derive(Clone, Copy)]
struct Palette {
    background: Color32,
    panel: Color32,
    field_group: Color32,
    text: Color32,
    muted: Color32,
    line: Color32,
    accent: Color32,
    success: Color32,
    warning: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: Color32::from_rgb(20, 26, 34),
                panel: Color32::from_rgb(27, 35, 47),
                field_group: Color32::from_rgb(24, 31, 42),
                text: Color32::from_rgb(232, 237, 244),
                muted: Color32::from_rgb(153, 168, 187),
                line: Color32::from_rgb(52, 65, 84),
                accent: Color32::from_rgb(123, 175, 218),
                success: Color32::from_rgb(137, 198, 181),
                warning: Color32::from_rgb(223, 183, 119),
            }
        } else {
            Self {
                background: Color32::from_rgb(241, 244, 248),
                panel: Color32::from_rgb(247, 249, 252),
                field_group: Color32::WHITE,
                text: Color32::from_rgb(32, 41, 54),
                muted: Color32::from_rgb(96, 112, 133),
                line: Color32::from_rgb(209, 217, 228),
                accent: Color32::from_rgb(36, 85, 174),
                success: Color32::from_rgb(37, 114, 96),
                warning: Color32::from_rgb(150, 96, 18),
            }
        }
    }
}

pub struct MatteApp {
    settings: Settings,
    config_path: Option<PathBuf>,
    scan: Option<ScanResult>,
    enabled: Vec<bool>,
    inspected: usize,
    query: String,
    scanning: bool,
    processing: bool,
    stop: Arc<AtomicBool>,
    tx: Sender<UiEvent>,
    rx: Receiver<UiEvent>,
    worker: Option<JoinHandle<()>>,
    per_sequence: BTreeMap<PathBuf, SequenceProgress>,
    total: usize,
    completed: usize,
    successful: usize,
    elapsed: f64,
    current: String,
    report: Option<BatchReport>,
    error: Option<String>,
    activity: VecDeque<String>,
    confirm_replace: bool,
    quit_when_idle: bool,
    logo: TextureHandle,
    initial_scan: bool,
    auto_run: bool,
    capture_dir: Option<PathBuf>,
    captured: [bool; 3],
    capture_pending: bool,
    exit_after_capture: bool,
    frames_drawn: usize,
}

impl MatteApp {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let (mut preferences, warning) = settings::load(args.config.as_deref());
        if let Some(source) = args.source {
            preferences.last_folder_path = source.display().to_string();
        }
        if let Some(output) = args.output_root {
            preferences.custom_output = true;
            preferences.output_root = output.display().to_string();
        }
        // Automated visual review always leaves original deliveries in place.
        if args.run {
            preferences.replace_originals = false;
        }
        set_theme(&cc.egui_ctx, preferences.dark_mode);
        let png = image::load_from_memory(include_bytes!("../images/icon.png"))
            .unwrap()
            .into_rgba8();
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [png.width() as usize, png.height() as usize],
            png.as_raw(),
        );
        let logo = cc
            .egui_ctx
            .load_texture("color-wheel", color, egui::TextureOptions::LINEAR);
        let (tx, rx) = mpsc::channel();
        Self {
            settings: preferences,
            config_path: args.config.or_else(settings::config_path),
            scan: None,
            enabled: Vec::new(),
            inspected: 0,
            query: String::new(),
            scanning: false,
            processing: false,
            stop: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            worker: None,
            per_sequence: BTreeMap::new(),
            total: 0,
            completed: 0,
            successful: 0,
            elapsed: 0.0,
            current: String::new(),
            report: None,
            error: warning,
            activity: VecDeque::new(),
            confirm_replace: false,
            quit_when_idle: false,
            logo,
            initial_scan: args.scan || args.run,
            auto_run: args.run,
            capture_dir: args.capture_dir,
            captured: [false; 3],
            capture_pending: false,
            exit_after_capture: args.exit_after_capture,
            frames_drawn: 0,
        }
    }

    fn busy(&self) -> bool {
        self.scanning || self.processing
    }

    fn output_root(&self) -> Option<PathBuf> {
        self.settings
            .custom_output
            .then(|| PathBuf::from(&self.settings.output_root))
    }

    fn log(&mut self, message: String) {
        self.activity.push_back(message);
        while self.activity.len() > 80 {
            self.activity.pop_front();
        }
    }

    fn persist(&mut self) {
        if let Some(path) = &self.config_path
            && let Err(error) = settings::save(&self.settings, path)
        {
            self.error = Some(format!("Could not save preferences: {error:#}"));
        }
    }

    fn reset_results(&mut self) {
        self.scan = None;
        self.enabled.clear();
        self.report = None;
        self.per_sequence.clear();
        self.completed = 0;
        self.successful = 0;
        self.total = 0;
        self.elapsed = 0.0;
        self.current.clear();
        self.captured = [false; 3];
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        self.reset_results();
        self.error = None;
        self.scanning = true;
        let root = PathBuf::from(&self.settings.last_folder_path);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.worker = Some(std::thread::spawn(move || {
            let result = sequences::scan(&root).map_err(|error| format!("{error:#}"));
            let _ = tx.send(UiEvent::Scan(result));
            ctx.request_repaint();
        }));
        self.persist();
    }

    fn start_batch(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let Some(scan) = &self.scan else {
            return;
        };
        let chosen = scan
            .sequences
            .iter()
            .zip(&self.enabled)
            .filter(|(_, enabled)| **enabled)
            .map(|(sequence, _)| sequence.clone())
            .collect();
        if self.settings.custom_output && self.settings.output_root.trim().is_empty() {
            self.error = Some("Choose a destination folder or use outputs beside sources.".into());
            return;
        }
        let options = BatchOptions {
            compression: self.settings.compression,
            matte_channel: self.settings.matte_channel_name.clone(),
            workers: self.settings.workers,
            output_root: self.output_root(),
            replace_originals: self.settings.replace_originals,
        };
        let plan = match Plan::new(chosen, options) {
            Ok(plan) => plan,
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                return;
            }
        };
        let executable = match std::env::current_exe() {
            Ok(path) => path,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        self.total = plan.jobs.len();
        self.completed = 0;
        self.successful = 0;
        self.elapsed = 0.0;
        self.processing = true;
        self.report = None;
        self.error = None;
        self.per_sequence.clear();
        self.stop = Arc::new(AtomicBool::new(false));
        self.log(format!(
            "Started {} frames using {} parallel workers.",
            self.total, self.settings.workers
        ));
        let stop = self.stop.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.worker = Some(std::thread::spawn(move || {
            let (batch_tx, batch_rx) = mpsc::channel();
            let runner = std::thread::spawn(move || batch::run(plan, &executable, stop, batch_tx));
            while let Ok(event) = batch_rx.recv() {
                let done = matches!(event, BatchEvent::Finished(_));
                let _ = tx.send(UiEvent::Batch(event));
                ctx.request_repaint();
                if done {
                    break;
                }
            }
            if runner.join().is_err() {
                let _ = tx.send(UiEvent::Failed(
                    "Batch worker stopped unexpectedly; originals remain in place.".into(),
                ));
            }
            ctx.request_repaint();
        }));
        self.persist();
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UiEvent::Scan(result) => {
                    self.scanning = false;
                    match result {
                        Ok(scan) => {
                            self.enabled = scan
                                .sequences
                                .iter()
                                .map(|sequence| sequence.ready())
                                .collect();
                            self.log(format!(
                                "Found {} sequences and {} frames.",
                                scan.sequences.len(),
                                scan.sequences.iter().map(|s| s.files.len()).sum::<usize>()
                            ));
                            self.scan = Some(scan);
                            self.inspected = 0;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                UiEvent::Batch(BatchEvent::Progress(progress)) => {
                    self.completed = self.completed.max(progress.completed);
                    self.successful = self.successful.max(progress.successful).min(self.completed);
                    self.elapsed = self.elapsed.max(progress.elapsed_seconds);
                    let counts = self
                        .per_sequence
                        .entry(progress.sequence.clone())
                        .or_default();
                    counts.completed += 1;
                    counts.failed += usize::from(progress.error.is_some());
                    let name = progress
                        .sequence
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    self.current = format!("{name}  frame {}", progress.frame);
                    if let Some(error) = progress.error {
                        self.log(format!("{name} / {}: {error}", progress.frame));
                    }
                }
                UiEvent::Batch(BatchEvent::Replacing(folder)) => {
                    self.current = format!(
                        "Replacing {}",
                        folder.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                UiEvent::Batch(BatchEvent::Finished(report)) => {
                    self.processing = false;
                    self.completed = report.completed;
                    self.successful = report.successful;
                    self.elapsed = report.elapsed_seconds;
                    self.log(format!(
                        "{} of {} frames embedded in {:.1}s. {} frame errors.",
                        report.successful,
                        report.total,
                        report.elapsed_seconds,
                        report.failures.len()
                    ));
                    for error in &report.replacement_errors {
                        self.log(error.clone());
                    }
                    self.report = Some(report);
                    if let Some(worker) = self.worker.take() {
                        let _ = worker.join();
                    }
                }
                UiEvent::Failed(error) => {
                    self.scanning = false;
                    self.processing = false;
                    self.error = Some(error);
                }
            }
        }
        if self.initial_scan {
            self.initial_scan = false;
            self.start_scan(ctx);
        }
        if !self.busy() && self.quit_when_idle {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn header(&mut self, ui: &mut Ui, palette: Palette) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(36.0, 36.0)));
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("EXR Matte Embed").size(20.0).strong());
                ui.label(
                    RichText::new("EXR delivery matte embedding")
                        .size(12.0)
                        .color(palette.muted),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if appearance_button(ui, self.settings.dark_mode, palette).clicked() {
                    self.settings.dark_mode = !self.settings.dark_mode;
                    set_theme(ui.ctx(), self.settings.dark_mode);
                    self.persist();
                }
                ui.label(RichText::new("2.0.0 beta").size(12.0).color(palette.muted));
            });
        });
    }

    fn controls(&mut self, ui: &mut Ui, palette: Palette) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();
        let mut changed = false;
        egui::Panel::bottom("embed-action")
            .exact_size(66.0)
            .frame(Frame::new().inner_margin(Margin::symmetric(0, 8)))
            .show(ui, |ui| {
                if busy {
                    let stopping = self.stop.load(Ordering::Acquire);
                    if ui
                        .add_enabled(
                            self.processing && !stopping,
                            egui::Button::new(if stopping {
                                "Stopping…"
                            } else if self.scanning {
                                "Scanning…"
                            } else {
                                "Stop after current frames"
                            })
                            .min_size(egui::vec2(ui.available_width(), 42.0)),
                        )
                        .clicked()
                    {
                        self.stop.store(true, Ordering::Release);
                    }
                } else {
                    let chosen = self.chosen_frames();
                    let can_run = chosen > 0
                        && sequences::validate_prefix(&self.settings.matte_channel_name).is_ok();
                    let button = egui::Button::new(
                        RichText::new(format!("Embed {chosen} frames"))
                            .strong()
                            .color(if self.settings.dark_mode {
                                palette.background
                            } else {
                                Color32::WHITE
                            }),
                    )
                    .fill(palette.accent)
                    .min_size(egui::vec2(ui.available_width(), 42.0));
                    if ui.add_enabled(can_run, button).clicked() {
                        if self.settings.replace_originals {
                            self.confirm_replace = true;
                        } else {
                            self.start_batch(&ctx);
                        }
                    }
                }
            });
        egui::ScrollArea::vertical().id_salt("controls-scroll").show(ui, |ui| {
            ui.label(RichText::new("Processing controls").size(15.0).strong());
            ui.add_space(6.0);
            ui.add_enabled_ui(!busy, |ui| {
                field_group(palette).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section(ui, "Source folder", palette);
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 78.0).max(100.0);
                        let source = ui.add(egui::TextEdit::singleline(&mut self.settings.last_folder_path)
                            .hint_text("Choose or drop a folder").desired_width(width));
                        if source.changed() { self.reset_results(); changed = true; }
                        source.on_hover_text(&self.settings.last_folder_path);
                        if ui.button(RichText::new("Browse").color(palette.accent)).clicked()
                            && let Some(path) = rfd::FileDialog::new().set_title("Choose a delivery folder").pick_folder() {
                            self.settings.last_folder_path = path.display().to_string();
                            self.reset_results(); changed = true;
                        }
                    });
                    let scan_button = egui::Button::new(RichText::new("Scan folder").color(palette.accent))
                        .min_size(egui::vec2(ui.available_width(), 30.0));
                    if ui.add_enabled(!self.settings.last_folder_path.trim().is_empty(), scan_button).clicked() {
                        self.start_scan(&ctx);
                    }
                });
                ui.add_space(8.0);
                field_group(palette).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section(ui, "Processing options", palette);
                    egui::Grid::new("processing-options").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        ui.label("Compression");
                        egui::ComboBox::from_id_salt("compression").width(146.0)
                            .selected_text(self.settings.compression.label()).show_ui(ui, |ui| {
                            for codec in Codec::ALL {
                                changed |= ui.selectable_value(&mut self.settings.compression, codec, codec.label()).changed();
                            }
                        });
                        ui.end_row();
                        ui.label("Matte channel name");
                        changed |= ui.add(egui::TextEdit::singleline(&mut self.settings.matte_channel_name)
                            .hint_text("matte").desired_width(146.0)).on_hover_text("Prefix for named mattes, e.g. DI_Matte.hero.").changed();
                        ui.end_row();
                        ui.label("Parallel frames");
                        changed |= ui.add(egui::DragValue::new(&mut self.settings.workers)
                            .range(1..=settings::cpu_count()).speed(0.2)).changed();
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new(if self.settings.compression.lossy() {
                        "Lossy compression can change image samples."
                    } else { "Lossless compression preserves image samples." }).size(12.0)
                        .color(if self.settings.compression.lossy() { palette.warning } else { palette.muted }));
                    ui.label(RichText::new(format!("{} codec threads per frame",
                        (settings::cpu_count() / self.settings.workers.max(1)).max(1))).size(12.0).color(palette.muted));
                    ui.add_space(4.0);
                    changed |= ui.add_enabled(!self.settings.custom_output,
                        egui::Checkbox::new(&mut self.settings.replace_originals, "Replace originals (move to Trash)"))
                        .on_hover_text("Publish completed sequences, then move original source and matte folders to Trash.").changed();
                    ui.label(RichText::new(if self.settings.replace_originals {
                        "You’ll review replacement before processing."
                    } else { "Original source and matte folders stay in place." }).size(12.0).color(palette.muted));
                });
                ui.add_space(8.0);
                field_group(palette).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section(ui, "Destination", palette);
                    ui.horizontal(|ui| {
                        changed |= ui.radio_value(&mut self.settings.custom_output, false, "Beside sources").changed();
                        changed |= ui.radio_value(&mut self.settings.custom_output, true, "Choose folder").changed();
                    });
                    if self.settings.custom_output {
                        self.settings.replace_originals = false;
                        ui.horizontal(|ui| {
                            let width = (ui.available_width() - 78.0).max(100.0);
                            changed |= ui.add(egui::TextEdit::singleline(&mut self.settings.output_root)
                                .hint_text("Destination root").desired_width(width)).on_hover_text(&self.settings.output_root).changed();
                            if ui.button(RichText::new("Browse").color(palette.accent)).clicked()
                                && let Some(path) = rfd::FileDialog::new().set_title("Choose the destination root").pick_folder() {
                                self.settings.output_root = path.display().to_string(); changed = true;
                            }
                        });
                    }
                    ui.label(RichText::new("One _embedded folder per sequence.").size(12.0).color(palette.muted));
                });
            });
        });
        if changed {
            self.persist();
        }
    }

    fn chosen_frames(&self) -> usize {
        self.scan
            .as_ref()
            .map(|scan| {
                scan.sequences
                    .iter()
                    .zip(&self.enabled)
                    .filter(|(_, enabled)| **enabled)
                    .map(|(sequence, _)| sequence.files.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    fn queue(&mut self, ui: &mut Ui, palette: Palette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scan results").size(18.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Find a sequence")
                        .desired_width(180.0),
                );
            });
        });
        let Some(scan) = &self.scan else {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                if self.scanning {
                    ui.spinner();
                    ui.heading("Scanning delivery folder…");
                } else {
                    ui.heading("Your sequences will appear here");
                    ui.label(
                        RichText::new("Choose a delivery folder and click Scan folder.")
                            .color(palette.muted),
                    );
                    ui.add_space(18.0);
                    ui.label("SHOT / SHOT_matte / SHOT_matteHero");
                    ui.label(
                        RichText::new("Matte red channels become named channels in your EXRs.")
                            .color(palette.muted),
                    );
                }
            });
            return;
        };
        ui.label(
            RichText::new(format!(
                "Found {} sequences with {} frames selected",
                scan.sequences.len(),
                self.chosen_frames()
            ))
            .color(palette.muted),
        );
        if scan.sequences.is_empty() {
            ui.add_space(40.0);
            ui.label("No matching EXR sequences found.");
            ui.label("Place numbered EXRs in a source folder with _matte* sibling folders.");
            return;
        }
        ui.add_space(8.0);
        let indices: Vec<_> = scan
            .sequences
            .iter()
            .enumerate()
            .filter(|(_, sequence)| {
                sequence
                    .name()
                    .to_lowercase()
                    .contains(&self.query.to_lowercase())
            })
            .map(|(index, _)| index)
            .collect();
        let height = (ui.available_height() - 210.0).max(100.0);
        let processing = self.processing;
        let show_channels = ui.available_width() > 650.0;
        let header_rect =
            egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 28.0));
        ui.painter().rect_filled(header_rect, 3.0, palette.panel);
        let mut table = TableBuilder::new(ui)
            .id_salt("delivery-table")
            .striped(true)
            .resizable(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(28.0))
            .column(Column::remainder().at_least(200.0).clip(true))
            .column(Column::exact(56.0));
        if show_channels {
            table = table.column(Column::exact(108.0).clip(true));
        }
        table
            .column(Column::exact(104.0))
            .max_scroll_height(height)
            .header(28.0, |mut header| {
                header.col(|ui| {
                    let mut all = self
                        .enabled
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| scan.sequences[*index].ready())
                        .all(|(_, enabled)| *enabled);
                    if ui
                        .add_enabled(!processing, egui::Checkbox::without_text(&mut all))
                        .changed()
                    {
                        for (sequence, enabled) in scan.sequences.iter().zip(&mut self.enabled) {
                            *enabled = all && sequence.ready();
                        }
                    }
                });
                let labels = if show_channels {
                    vec!["Sequence", "Frames", "Matte channels", "Status"]
                } else {
                    vec!["Sequence", "Frames", "Status"]
                };
                for name in labels {
                    header.col(|ui| {
                        ui.label(RichText::new(name).color(palette.muted));
                    });
                }
            })
            .body(|body| {
                body.rows(28.0, indices.len(), |mut row| {
                    let index = indices[row.index()];
                    let sequence = &scan.sequences[index];
                    row.col(|ui| {
                        ui.add_enabled(
                            !processing && sequence.ready(),
                            egui::Checkbox::without_text(&mut self.enabled[index]),
                        );
                    });
                    row.col(|ui| {
                        let response = ui.add(
                            egui::Button::selectable(
                                self.inspected == index,
                                RichText::new(sequence.name()).size(14.0),
                            )
                            .frame(false),
                        );
                        if response.clicked() {
                            self.inspected = index;
                        }
                        response.on_hover_text(sequence.folder.display().to_string());
                    });
                    row.col(|ui| {
                        ui.label(sequence.files.len().to_string());
                    });
                    if show_channels {
                        row.col(|ui| {
                            let names = sequence
                                .channel_names(&self.settings.matte_channel_name)
                                .unwrap_or_default();
                            ui.add(
                                egui::Label::new(if names.len() == 1 {
                                    names[0].clone()
                                } else {
                                    format!("{} channels", names.len())
                                })
                                .truncate(),
                            )
                            .on_hover_text(names.join(", "));
                        });
                    }
                    row.col(|ui| {
                        let counts = self.per_sequence.get(&sequence.folder);
                        let (text, color) = if !sequence.ready() {
                            ("Needs attention".to_owned(), palette.warning)
                        } else if let Some(counts) = counts {
                            if counts.failed > 0 {
                                (format!("{} errors", counts.failed), palette.warning)
                            } else if counts.completed == sequence.files.len() {
                                ("Embedded".to_owned(), palette.success)
                            } else {
                                (
                                    format!("{}/{}", counts.completed, sequence.files.len()),
                                    palette.accent,
                                )
                            }
                        } else if sequence.existing_outputs > 0 && !self.settings.custom_output {
                            ("Output exists".to_owned(), palette.warning)
                        } else if processing && self.enabled[index] {
                            ("Queued".to_owned(), palette.muted)
                        } else {
                            ("Ready".to_owned(), palette.success)
                        };
                        ui.label(RichText::new(text).size(12.0).color(color));
                    });
                });
            });
        ui.add_space(14.0);
        egui::ScrollArea::vertical()
            .id_salt("sequence-inspector")
            .show(ui, |ui| {
                field_group(palette).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.inspector(ui, palette);
                });
            });
    }

    fn inspector(&mut self, ui: &mut Ui, palette: Palette) {
        let Some(scan) = &self.scan else {
            return;
        };
        let Some(sequence) = scan.sequences.get(self.inspected) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(sequence.name()).size(15.0).strong());
            if let Some(info) = &sequence.image {
                ui.label(
                    RichText::new(format!("{} × {}", info.width, info.height)).color(palette.muted),
                );
            }
            if let (Some(first), Some(last)) = (
                sequence.files.keys().next(),
                sequence.files.keys().next_back(),
            ) {
                ui.label(
                    RichText::new(format!("Frames {first}–{last}"))
                        .size(12.0)
                        .color(palette.muted),
                );
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Keep").color(palette.muted));
            if let Some(info) = &sequence.image {
                for channel in &info.channels {
                    badge(ui, channel, palette.muted);
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Embed").color(palette.muted));
            match sequence.channel_names(&self.settings.matte_channel_name) {
                Ok(names) => {
                    for name in names {
                        badge(ui, &name, palette.accent);
                    }
                }
                Err(error) => {
                    ui.label(RichText::new(error.to_string()).color(palette.warning));
                }
            }
        });
        let output = sequence.output_folder(self.output_root().as_deref());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Output").size(12.0).color(palette.muted));
            ui.add(
                egui::Label::new(RichText::new(output.display().to_string()).size(12.0)).truncate(),
            )
            .on_hover_text(output.display().to_string());
        });
        for matte in &sequence.mattes {
            let name = sequences::channel_name(&self.settings.matte_channel_name, &matte.suffix);
            ui.label(
                RichText::new(format!(
                    "{name}  from {} / R",
                    matte
                        .folder
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ))
                .size(12.0)
                .color(palette.muted),
            );
        }
        for issue in &sequence.issues {
            ui.label(RichText::new(issue).size(12.0).color(palette.warning));
        }
        if sequence.existing_outputs > 0 && !self.settings.custom_output {
            ui.label(
                RichText::new(format!(
                    "{} outputs already exist. Choose a new destination to keep them.",
                    sequence.existing_outputs
                ))
                .size(12.0)
                .color(palette.warning),
            );
        }
        if !scan.warnings.is_empty() {
            ui.collapsing(format!("{} scan notices", scan.warnings.len()), |ui| {
                for warning in &scan.warnings {
                    ui.label(RichText::new(warning).color(palette.warning));
                }
            });
        }
        ui.collapsing("Batch details", |ui| {
            egui::ScrollArea::vertical()
                .max_height(100.0)
                .id_salt("activity")
                .show(ui, |ui| {
                    for entry in &self.activity {
                        ui.label(RichText::new(entry).size(12.0).color(palette.muted));
                    }
                    if let Some(report) = &self.report {
                        for failure in &report.failures {
                            ui.label(
                                RichText::new(format!(
                                    "Frame {}: {}",
                                    failure.frame, failure.error
                                ))
                                .color(palette.warning),
                            );
                        }
                    }
                });
        });
    }

    fn footer(&mut self, ui: &mut Ui, palette: Palette) {
        let (title, color) = if self.scanning {
            ("Scanning sequences", palette.accent)
        } else if self.processing && self.stop.load(Ordering::Acquire) {
            ("Stopping after current frames", palette.warning)
        } else if self.processing {
            ("Embedding mattes", palette.accent)
        } else if let Some(report) = &self.report {
            if report.cancelled {
                ("Batch stopped", palette.warning)
            } else if report.success() {
                ("Batch complete", palette.success)
            } else {
                ("Batch finished with errors", palette.warning)
            }
        } else if self.chosen_frames() > 0 {
            ("Ready to embed", palette.success)
        } else {
            ("Choose a folder to begin", palette.muted)
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(16.0).strong().color(color));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.report.is_some() {
                    if ui.button("Save report…").clicked() {
                        self.export_report();
                    }
                    if ui.button("Open output folder").clicked() {
                        let path = self
                            .output_root()
                            .or_else(|| self.scan.as_ref().map(|scan| scan.root.clone()));
                        if let Some(path) = path
                            && let Err(error) = open::that(path)
                        {
                            self.error = Some(error.to_string());
                        }
                    }
                }
                if self.total > 0 {
                    ui.label(format!("{} / {} frames", self.successful, self.total));
                }
            });
        });
        ui.add_space(5.0);
        let progress = if self.total > 0 {
            self.completed as f32 / self.total as f32
        } else {
            0.0
        };
        ui.add(
            egui::ProgressBar::new(progress)
                .desired_width(ui.available_width())
                .desired_height(8.0)
                .fill(
                    if self.report.as_ref().is_some_and(|report| report.success()) {
                        palette.success
                    } else {
                        palette.accent
                    },
                ),
        );
        ui.add_space(7.0);
        ui.columns(2, |columns| {
            let note = if self.processing {
                self.current.clone()
            } else if let Some(report) = &self.report {
                if !report.replacement_errors.is_empty() {
                    "Embedded files saved. Review replacement errors in Batch details.".into()
                } else if report.replaced.is_empty() {
                    format!(
                        "{} embedded, {} frame errors. Originals kept.",
                        report.successful,
                        report.failures.len()
                    )
                } else {
                    format!(
                        "{} sequences replaced; originals moved to Trash.",
                        report.replaced.len()
                    )
                }
            } else {
                "Matte R is stored as HALF. Existing image channels and metadata are preserved."
                    .into()
            };
            columns[0].add(
                egui::Label::new(RichText::new(note).size(12.0).color(palette.muted)).truncate(),
            );
            columns[1].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.completed > 0 {
                    if self.processing {
                        let remaining = self.elapsed / self.completed as f64
                            * self.total.saturating_sub(self.completed) as f64;
                        ui.label(
                            RichText::new(format!("{} remaining", duration(remaining)))
                                .size(12.0)
                                .color(palette.muted),
                        );
                    }
                    ui.label(
                        RichText::new(format!(
                            "{:.1} frames/s",
                            self.completed as f64 / self.elapsed.max(0.001)
                        ))
                        .size(12.0)
                        .color(palette.muted),
                    );
                    ui.label(
                        RichText::new(format!("Elapsed {}", duration(self.elapsed)))
                            .size(12.0)
                            .color(palette.muted),
                    );
                }
            });
        });
    }

    fn export_report(&mut self) {
        let Some(report) = &self.report else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("exr-matte-embed-report.json")
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return;
        };
        let outcome = (|| -> anyhow::Result<()> {
            let parent = path.parent().unwrap_or(Path::new("."));
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer_pretty(temporary.as_file_mut(), report)?;
            temporary.persist(&path)?;
            Ok(())
        })();
        if let Err(error) = outcome {
            self.error = Some(format!("Could not save report: {error:#}"));
        }
    }

    fn captures(&mut self, ctx: &egui::Context) {
        for event in ctx.input(|input| input.events.clone()) {
            if let egui::Event::Screenshot {
                image, user_data, ..
            } = event
            {
                let Some(stage) = user_data
                    .data
                    .as_ref()
                    .and_then(|data| data.downcast_ref::<usize>())
                    .copied()
                else {
                    continue;
                };
                let Some(root) = &self.capture_dir else {
                    continue;
                };
                let name = ["ready.png", "processing.png", "complete.png"][stage];
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|color| color.to_array())
                    .collect();
                let result = std::fs::create_dir_all(root)
                    .map_err(anyhow::Error::from)
                    .and_then(|()| {
                        image::save_buffer(
                            root.join(name),
                            &bytes,
                            image.size[0] as u32,
                            image.size[1] as u32,
                            image::ColorType::Rgba8,
                        )
                        .map_err(anyhow::Error::from)
                    });
                self.capture_pending = false;
                if let Err(error) = result {
                    self.error = Some(format!("Screenshot failed: {error}"));
                    self.auto_run = false;
                } else {
                    self.captured[stage] = true;
                    println!("Screenshot: {}", root.join(name).display());
                }
            }
        }
        if self.capture_dir.is_some() && !self.capture_pending && self.frames_drawn > 3 {
            let stage: Option<usize> = if !self.captured[0] && self.scan.is_some() && !self.busy() {
                Some(0)
            } else if !self.captured[1] && self.processing && self.completed > 0 {
                Some(1)
            } else if !self.captured[2] && self.report.is_some() {
                Some(2)
            } else {
                None
            };
            if let Some(stage) = stage {
                self.capture_pending = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    stage,
                )));
                ctx.request_repaint();
            }
        }
        if self.auto_run
            && !self.quit_when_idle
            && self.scan.is_some()
            && !self.busy()
            && (self.capture_dir.is_none() || self.captured[0])
        {
            self.auto_run = false;
            self.start_batch(ctx);
        }
        if self.exit_after_capture
            && !self.busy()
            && ((self.report.is_some() && self.captured[2])
                || (self.report.is_none() && !self.auto_run && self.captured[0]))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for MatteApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if ctx.input(|input| input.viewport().close_requested()) && self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.stop.store(true, Ordering::Release);
            self.quit_when_idle = true;
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let palette = Palette::new(self.settings.dark_mode);
        self.frames_drawn += 1;
        if !self.busy() {
            let dropped = ctx.input(|input| {
                input
                    .raw
                    .dropped_files
                    .first()
                    .map(|file| file.path().to_path_buf())
            });
            if let Some(folder) = dropped.filter(|path| path.is_dir()) {
                self.settings.last_folder_path = folder.display().to_string();
                self.start_scan(&ctx);
            }
        }
        egui::Panel::top("header")
            .exact_size(68.0)
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(18, 12)),
            )
            .show(ui, |ui| self.header(ui, palette));
        egui::Panel::bottom("progress")
            .exact_size(104.0)
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(22, 12)),
            )
            .show(ui, |ui| self.footer(ui, palette));
        egui::Panel::left("controls")
            .exact_size(350.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| self.controls(ui, palette));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(palette.background).inner_margin(18))
            .show(ui, |ui| {
                if let Some(error) = self.error.clone() {
                    Frame::new()
                        .fill(palette.warning.gamma_multiply(0.12))
                        .inner_margin(10)
                        .corner_radius(4)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(error).color(palette.warning));
                                if ui.small_button("Dismiss").clicked() {
                                    self.error = None;
                                }
                            });
                        });
                    ui.add_space(10.0);
                }
                self.queue(ui, palette);
            });
        if self.confirm_replace {
            egui::Modal::new(egui::Id::new("replace-confirmation")).show(&ctx, |ui| {
                ui.set_width(440.0); ui.heading("Replace the selected original sequences?");
                ui.label("After every frame embeds successfully, the app will publish the embedded sequences and move the original source and matte folders to Trash.");
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep originals").clicked() { self.confirm_replace = false; }
                    if ui.button("Embed and replace").clicked() { self.confirm_replace = false; self.start_batch(&ctx); }
                });
            });
        }
        self.captures(&ctx);
        if self.busy() || self.capture_pending || (self.capture_dir.is_some() && !self.captured[0])
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
    }
}

fn section(ui: &mut Ui, name: &str, palette: Palette) {
    ui.label(RichText::new(name).size(14.0).strong().color(palette.text));
    ui.add_space(5.0);
}

fn field_group(palette: Palette) -> Frame {
    Frame::new()
        .fill(palette.field_group)
        .stroke(Stroke::new(1.0, palette.line))
        .corner_radius(5)
        .inner_margin(10)
}

fn appearance_button(ui: &mut Ui, dark: bool, palette: Palette) -> egui::Response {
    let label = if dark {
        "Use light appearance"
    } else {
        "Use dark appearance"
    };
    let response = ui.add(
        egui::Button::new("")
            .min_size(egui::vec2(32.0, 32.0))
            .corner_radius(5),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let center = response.rect.center();
    let stroke = Stroke::new(1.5, palette.text);
    if dark {
        ui.painter().circle_stroke(center, 4.5, stroke);
        for ray in 0..8 {
            let direction = egui::Vec2::angled(ray as f32 * std::f32::consts::TAU / 8.0);
            ui.painter().line_segment(
                [center + direction * 7.0, center + direction * 10.0],
                stroke,
            );
        }
    } else {
        for offsets in [
            [[4.0, -8.0], [-9.0, -7.0], [-9.0, 8.0], [4.0, 8.0]],
            [[4.0, 8.0], [-4.0, 5.0], [-4.0, -4.0], [4.0, -8.0]],
        ] {
            ui.painter()
                .add(egui::epaint::CubicBezierShape::from_points_stroke(
                    offsets.map(|[x, y]| center + egui::vec2(x, y)),
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                ));
        }
    }
    response.on_hover_text(label)
}

fn badge(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.3)))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}

fn duration(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}m {:02}s", seconds as u64 / 60, seconds as u64 % 60)
    }
}

fn set_theme(ctx: &egui::Context, dark: bool) {
    let palette = Palette::new(dark);
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.override_text_color = Some(palette.text);
    visuals.panel_fill = palette.panel;
    visuals.window_fill = palette.panel;
    visuals.extreme_bg_color = palette.background;
    visuals.selection.bg_fill = palette.accent.gamma_multiply(0.22);
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.line);
    ctx.set_visuals(visuals);
    ctx.style_mut_of(theme, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 5.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        for text in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style
                .text_styles
                .insert(text, egui::FontId::proportional(14.0));
        }
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(21.0));
    });
}
