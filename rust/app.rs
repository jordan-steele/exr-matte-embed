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
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

egui_phosphor::subset! {
    mod icons {
        use regular::{SUN, MOON, FOLDER_OPEN, CARET_DOWN, CARET_RIGHT};
    }
}

const ICON_FONT: &str = "app-icons";

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
enum QueueRow {
    Group(usize, usize),
    Sequence(usize),
    Source(usize),
    Matte(usize, usize),
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
                background: Color32::from_rgb(32, 34, 37),
                panel: Color32::from_rgb(38, 40, 43),
                field_group: Color32::from_rgb(35, 37, 40),
                text: Color32::from_rgb(234, 236, 239),
                muted: Color32::from_rgb(166, 170, 177),
                line: Color32::from_rgb(65, 68, 73),
                accent: Color32::from_rgb(116, 175, 245),
                success: Color32::from_rgb(137, 193, 165),
                warning: Color32::from_rgb(232, 183, 108),
            }
        } else {
            Self {
                background: Color32::from_rgb(248, 249, 250),
                panel: Color32::from_rgb(241, 242, 244),
                field_group: Color32::WHITE,
                text: Color32::from_rgb(39, 43, 49),
                muted: Color32::from_rgb(104, 111, 121),
                line: Color32::from_rgb(211, 214, 220),
                accent: Color32::from_rgb(36, 109, 197),
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
    expanded: BTreeSet<usize>,
    groups_open: [bool; 2],
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
        set_fonts(&cc.egui_ctx);
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
            expanded: BTreeSet::new(),
            groups_open: [true; 2],
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
        self.expanded.clear();
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
                            self.expanded = (0..if scan.sequences.len() <= 3 {
                                scan.sequences.len()
                            } else {
                                1
                            })
                                .collect();
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
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(26.0, 26.0)));
            ui.add_space(3.0);
            ui.label(RichText::new("EXR Matte Embed").size(16.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if appearance_button(ui, self.settings.dark_mode, palette).clicked() {
                    self.settings.dark_mode = !self.settings.dark_mode;
                    set_theme(ui.ctx(), self.settings.dark_mode);
                    self.persist();
                }
                ui.label(RichText::new("2.0.0 beta").size(11.0).color(palette.muted));
            });
        });
    }

    fn controls(&mut self, ui: &mut Ui, palette: Palette) {
        let busy = self.busy();
        let ctx = ui.ctx().clone();
        let mut changed = false;
        if busy {
            egui::Panel::bottom("running-action")
                .exact_size(46.0)
                .frame(Frame::new().inner_margin(Margin::symmetric(0, 6)))
                .show(ui, |ui| {
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
                            .min_size(egui::vec2(ui.available_width(), 34.0)),
                        )
                        .clicked()
                    {
                        self.stop.store(true, Ordering::Release);
                    }
                });
        }
        egui::ScrollArea::vertical().id_salt("controls-scroll").show(ui, |ui| {
            ui.horizontal(|ui| { ui.label(RichText::new("Processing controls").size(14.0).strong()); });
            ui.add_space(9.0);
            ui.add_enabled_ui(!busy, |ui| {
                field_group(palette).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section(ui, "Source folder", palette);
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 41.0).max(100.0);
                        let source = ui.add(egui::TextEdit::singleline(&mut self.settings.last_folder_path)
                            .hint_text("Choose or drop a folder").desired_width(width));
                        if source.changed() { self.reset_results(); changed = true; }
                        source.on_hover_text(&self.settings.last_folder_path);
                        if folder_button(ui, "Choose source folder", palette).clicked()
                            && let Some(path) = rfd::FileDialog::new().set_title("Choose a delivery folder").pick_folder() {
                            self.settings.last_folder_path = path.display().to_string();
                            self.reset_results(); changed = true;
                        }
                    });
                    ui.add_space(2.0);
                    let scan_button = egui::Button::new((egui::Atom::grow(), RichText::new("Scan folder").color(palette.accent), egui::Atom::grow()))
                        .min_size(egui::vec2(ui.available_width(), 28.0));
                    if ui.add_enabled(!self.settings.last_folder_path.trim().is_empty(), scan_button).clicked() {
                        self.start_scan(&ctx);
                    }
                });
                ui.add_space(8.0);
                field_group(palette).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    section(ui, "Processing options", palette);
                    let control_width = (ui.available_width() - 145.0).max(108.0);
                    egui::Grid::new("processing-options").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                        ui.label("Compression");
                        egui::ComboBox::from_id_salt("compression").width(control_width)
                            .selected_text(self.settings.compression.label()).show_ui(ui, |ui| {
                            for codec in Codec::ALL {
                                changed |= ui.selectable_value(&mut self.settings.compression, codec, codec.label()).changed();
                            }
                        }).response.on_hover_text("Lossless codecs preserve every image sample. DWAA, DWAB and B44 codecs are lossy.");
                        ui.end_row();
                        ui.label("Matte channel name");
                        changed |= ui.add(egui::TextEdit::singleline(&mut self.settings.matte_channel_name)
                            .hint_text("matte").desired_width(control_width)).on_hover_text("Prefix for named mattes, e.g. matte.hero.").changed();
                        ui.end_row();
                        ui.label("Parallel frames");
                        changed |= ui.add(egui::DragValue::new(&mut self.settings.workers)
                            .range(1..=settings::cpu_count()).speed(0.2))
                            .on_hover_text(format!("Frames processed at once. {} codec threads per frame.",
                                (settings::cpu_count() / self.settings.workers.max(1)).max(1))).changed();
                        ui.end_row();
                    });
                    if self.settings.compression.lossy() {
                        ui.add_space(4.0);
                        ui.label(RichText::new("Lossy compression can change image samples.").size(11.0).color(palette.warning));
                    }
                    if let Err(error) = sequences::validate_prefix(&self.settings.matte_channel_name) {
                        ui.label(RichText::new(error.to_string()).size(11.0).color(palette.warning));
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(3.0);
                    changed |= ui.add_enabled(!self.settings.custom_output,
                        egui::Checkbox::new(&mut self.settings.replace_originals, "Replace originals (move to Trash)"))
                        .on_hover_text("After every frame succeeds, publish completed sequences and move original source and matte folders to Trash.").changed();
                    if self.settings.replace_originals {
                        ui.label(RichText::new("Review replacement before processing.").size(11.0).color(palette.warning));
                    }
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
                            let width = (ui.available_width() - 41.0).max(100.0);
                            changed |= ui.add(egui::TextEdit::singleline(&mut self.settings.output_root)
                                .hint_text("Destination root").desired_width(width)).on_hover_text(&self.settings.output_root).changed();
                            if folder_button(ui, "Choose destination folder", palette).clicked()
                                && let Some(path) = rfd::FileDialog::new().set_title("Choose the destination root").pick_folder() {
                                self.settings.output_root = path.display().to_string(); changed = true;
                            }
                        });
                    }
                    ui.label(RichText::new("Saves each sequence in an _embedded folder.").size(11.0).color(palette.muted));
                });
            });
            ui.add_space(10.0);
            if !busy {
                let chosen = self.chosen_frames();
                let can_run = chosen > 0 && sequences::validate_prefix(&self.settings.matte_channel_name).is_ok();
                let label = if chosen > 0 { format!("Embed {} frames", number(chosen)) } else { "Embed sequences".into() };
                let button = egui::Button::new((egui::Atom::grow(), RichText::new(label).strong().color(Color32::WHITE), egui::Atom::grow()))
                    .fill(Color32::from_rgb(44, 117, 205))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(66, 137, 224)))
                    .min_size(egui::vec2(ui.available_width(), 34.0));
                if ui.add_enabled(can_run, button).clicked() {
                    if self.settings.replace_originals { self.confirm_replace = true; }
                    else { self.start_batch(&ctx); }
                }
            }
            ui.add_space(10.0);
            field_group(palette).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.footer(ui, palette);
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
        let mut filter_changed = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scan results").size(14.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                filter_changed = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.query)
                            .hint_text("Filter sequences…")
                            .desired_width(174.0),
                    )
                    .changed();
            });
        });
        let Some(scan) = &self.scan else {
            ui.add_space(100.0);
            ui.vertical_centered(|ui| {
                if self.scanning {
                    ui.spinner();
                    ui.label(RichText::new("Scanning delivery folder…").size(16.0));
                } else {
                    ui.label(
                        RichText::new("Choose a folder to get started")
                            .size(16.0)
                            .strong(),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "Drop a delivery folder here, or browse in Processing controls.",
                        )
                        .color(palette.muted),
                    );
                    ui.add_space(24.0);
                    ui.label(
                        RichText::new("Source and matching _matte folders will appear together.")
                            .size(12.0)
                            .color(palette.muted),
                    );
                }
            });
            return;
        };
        let query = self.query.trim().to_lowercase();
        let indices: Vec<_> = scan
            .sequences
            .iter()
            .enumerate()
            .filter(|(_, sequence)| {
                sequence.name().to_lowercase().contains(&query)
                    || sequence.mattes.iter().any(|matte| {
                        matte
                            .folder
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query)
                    })
            })
            .map(|(index, _)| index)
            .collect();
        if filter_changed && !query.is_empty() {
            self.expanded.extend(indices.iter().copied());
            self.groups_open = [true; 2];
            if !indices.contains(&self.inspected)
                && let Some(&index) = indices.first()
            {
                self.inspected = index;
            }
        }
        let ready_indices: Vec<_> = indices
            .iter()
            .copied()
            .filter(|&index| scan.sequences[index].ready())
            .collect();
        let selected = self.enabled.iter().filter(|enabled| **enabled).count();
        ui.label(
            RichText::new(if query.is_empty() {
                format!(
                    "{} sequences  /  {} frames selected",
                    scan.sequences.len(),
                    number(self.chosen_frames())
                )
            } else {
                format!(
                    "{} of {} sequences shown  /  {} selected",
                    indices.len(),
                    scan.sequences.len(),
                    selected
                )
            })
            .size(12.0)
            .color(palette.muted),
        );
        ui.add_space(10.0);
        if scan.sequences.is_empty() {
            ui.label("No matching EXR sequences found.");
            ui.label("Place numbered EXRs in a source folder with _matte* sibling folders.");
            return;
        }
        let mut rows = Vec::new();
        for group in 0..2 {
            let members: Vec<_> = indices
                .iter()
                .copied()
                .filter(|&index| usize::from(scan.sequences[index].mattes.len() > 1) == group)
                .collect();
            if members.is_empty() {
                continue;
            }
            rows.push(QueueRow::Group(group, members.len()));
            if self.groups_open[group] {
                for index in members {
                    rows.push(QueueRow::Sequence(index));
                    if self.expanded.contains(&index) {
                        rows.push(QueueRow::Source(index));
                        for matte in 0..scan.sequences[index].mattes.len() {
                            rows.push(QueueRow::Matte(index, matte));
                        }
                    }
                }
            }
        }
        let height = (ui.available_height() - 179.0).max(100.0);
        let show_type = ui.available_width() > 620.0;
        let header_rect =
            egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 28.0));
        ui.painter().rect_filled(header_rect, 3.0, palette.panel);
        let mut table = TableBuilder::new(ui)
            .id_salt("delivery-tree")
            .auto_shrink([false, false])
            .sense(egui::Sense::click())
            .striped(true)
            .resizable(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder().at_least(180.0).clip(true));
        if show_type {
            table = table.column(Column::exact(105.0).clip(true));
        }
        table
            .column(Column::exact(48.0))
            .column(Column::exact(100.0))
            .min_scrolled_height(height)
            .max_scroll_height(height)
            .header(28.0, |mut header| {
                header.col(|ui| {
                    ui.add_space(23.0);
                    let mut all =
                        !ready_indices.is_empty() && ready_indices.iter().all(|&i| self.enabled[i]);
                    if ui
                        .add_enabled(
                            !self.processing && !ready_indices.is_empty(),
                            egui::Checkbox::without_text(&mut all),
                        )
                        .on_hover_text("Select all visible ready sequences")
                        .changed()
                    {
                        for &index in &indices {
                            self.enabled[index] = all && scan.sequences[index].ready();
                        }
                    }
                    ui.label(
                        RichText::new("Sequence / channel")
                            .size(12.0)
                            .color(palette.muted),
                    );
                });
                if show_type {
                    header.col(|ui| {
                        ui.label(RichText::new("Type").size(12.0).color(palette.muted));
                    });
                }
                header.col(|ui| {
                    ui.label(RichText::new("Frames").size(12.0).color(palette.muted));
                });
                header.col(|ui| {
                    ui.label(RichText::new("Status").size(12.0).color(palette.muted));
                });
            })
            .body(|body| {
                body.rows(25.0, rows.len(), |mut row| {
                    let entry = rows[row.index()];
                    if let QueueRow::Sequence(index) = entry {
                        row.set_selected(index == self.inspected);
                    }
                    row.col(|ui| match entry {
                        QueueRow::Group(group, count) => {
                            let open = self.groups_open[group];
                            if disclosure_button(ui, open, "Toggle sequence group", palette)
                                .clicked()
                            {
                                self.groups_open[group] = !self.groups_open[group];
                            }
                            let label = if group == 0 {
                                "Single-channel mattes"
                            } else {
                                "Multi-channel mattes"
                            };
                            let response = ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{label} ({count})"))
                                        .size(12.0)
                                        .strong(),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            );
                            if response.clicked() {
                                self.groups_open[group] = !self.groups_open[group];
                            }
                        }
                        QueueRow::Sequence(index) => {
                            let sequence = &scan.sequences[index];
                            let open = self.expanded.contains(&index);
                            if disclosure_button(
                                ui,
                                open,
                                &format!("Show channels for {}", sequence.name()),
                                palette,
                            )
                            .clicked()
                                && !self.expanded.remove(&index)
                            {
                                self.expanded.insert(index);
                            }
                            ui.add_enabled(
                                !self.processing && sequence.ready(),
                                egui::Checkbox::without_text(&mut self.enabled[index]),
                            )
                            .on_hover_text("Include this sequence");
                            let response = ui.add(
                                egui::Label::new(RichText::new(sequence.name()).size(13.0))
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            );
                            if response.clicked() {
                                self.inspected = index;
                            }
                            if response.double_clicked() && !self.expanded.remove(&index) {
                                self.expanded.insert(index);
                            }
                            response.on_hover_text(sequence.folder.display().to_string());
                        }
                        QueueRow::Source(index) | QueueRow::Matte(index, _) => {
                            let sequence = &scan.sequences[index];
                            let (label, path) = match entry {
                                QueueRow::Matte(_, matte) => {
                                    let matte = &sequence.mattes[matte];
                                    (
                                        sequences::channel_name(
                                            &self.settings.matte_channel_name,
                                            &matte.suffix,
                                        ),
                                        &matte.folder,
                                    )
                                }
                                _ => ("Source".into(), &sequence.folder),
                            };
                            let (guide, _) = ui
                                .allocate_exact_size(egui::vec2(49.0, 25.0), egui::Sense::hover());
                            let x = guide.left() + 32.0;
                            let stroke = Stroke::new(1.0, palette.line);
                            ui.painter().line_segment(
                                [egui::pos2(x, guide.top()), egui::pos2(x, guide.bottom())],
                                stroke,
                            );
                            ui.painter().line_segment(
                                [
                                    egui::pos2(x, guide.center().y),
                                    egui::pos2(x + 9.0, guide.center().y),
                                ],
                                stroke,
                            );
                            let response = ui.add(
                                egui::Label::new(RichText::new(label).size(12.0).color(
                                    if matches!(entry, QueueRow::Source(_)) {
                                        palette.muted
                                    } else {
                                        palette.accent
                                    },
                                ))
                                .truncate()
                                .sense(egui::Sense::click()),
                            );
                            if response.clicked() {
                                self.inspected = index;
                            }
                            response.on_hover_text(path.display().to_string());
                        }
                    });
                    if show_type {
                        row.col(|ui| {
                            let text = match entry {
                                QueueRow::Sequence(index) => format!(
                                    "{} matte{}",
                                    scan.sequences[index].mattes.len(),
                                    if scan.sequences[index].mattes.len() == 1 {
                                        ""
                                    } else {
                                        "s"
                                    }
                                ),
                                QueueRow::Source(index) => scan.sequences[index]
                                    .image
                                    .as_ref()
                                    .map(|info| info.channels.join(", "))
                                    .unwrap_or_default(),
                                QueueRow::Matte(_, _) => "Matte / R".into(),
                                _ => String::new(),
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&text).size(11.0).color(palette.muted),
                                )
                                .truncate(),
                            )
                            .on_hover_text(text);
                        });
                    }
                    row.col(|ui| {
                        if let QueueRow::Sequence(index) = entry {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(number(scan.sequences[index].files.len()))
                                            .size(12.0),
                                    );
                                },
                            );
                        }
                    });
                    row.col(|ui| {
                        if let QueueRow::Sequence(index) = entry {
                            let sequence = &scan.sequences[index];
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
                                        format!("{} / {}", counts.completed, sequence.files.len()),
                                        palette.accent,
                                    )
                                }
                            } else if sequence.existing_outputs > 0 && !self.settings.custom_output
                            {
                                ("Output exists".to_owned(), palette.warning)
                            } else if self.processing && self.enabled[index] {
                                ("Queued".to_owned(), palette.muted)
                            } else if !self.enabled[index] {
                                ("Excluded".to_owned(), palette.muted)
                            } else {
                                ("Ready".to_owned(), palette.muted)
                            };
                            ui.label(RichText::new(text).size(11.0).color(color));
                        }
                    });
                    if row.response().clicked() {
                        match entry {
                            QueueRow::Sequence(index)
                            | QueueRow::Source(index)
                            | QueueRow::Matte(index, _) => self.inspected = index,
                            QueueRow::Group(_, _) => {}
                        }
                    }
                });
            });
        if indices.is_empty() {
            ui.label(RichText::new("No sequences match this filter.").color(palette.muted));
        }
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("sequence-inspector")
            .show(ui, |ui| self.inspector(ui, palette));
    }

    fn inspector(&mut self, ui: &mut Ui, palette: Palette) {
        let Some(scan) = &self.scan else {
            return;
        };
        let Some(sequence) = scan.sequences.get(self.inspected) else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(sequence.name()).size(13.0).strong());
            if let Some(info) = &sequence.image {
                ui.label(
                    RichText::new(format!("{} × {}", info.width, info.height))
                        .size(11.0)
                        .color(palette.muted),
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
        section(ui, "Progress", palette);
        let (title, color) = if self.scanning {
            ("Scanning sequences…", palette.accent)
        } else if self.processing && self.stop.load(Ordering::Acquire) {
            ("Stopping after current frames…", palette.warning)
        } else if self.processing {
            ("Embedding mattes", palette.accent)
        } else if let Some(report) = &self.report {
            if report.cancelled {
                ("Batch stopped", palette.warning)
            } else if report.success() {
                ("Batch complete", palette.success)
            } else {
                ("Finished with errors", palette.warning)
            }
        } else if self.chosen_frames() > 0 {
            ("Ready to embed", palette.muted)
        } else {
            ("Waiting for a folder", palette.muted)
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(12.0).color(color));
            if self.total > 0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{:.0}%",
                            100.0 * self.completed as f64 / self.total as f64
                        ))
                        .size(12.0)
                        .strong(),
                    );
                });
            }
        });
        ui.add_space(3.0);
        let progress = if self.total > 0 {
            self.completed as f32 / self.total as f32
        } else {
            0.0
        };
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::hover());
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(
                egui::WidgetType::ProgressIndicator,
                ui.is_enabled(),
                "Embedding progress",
            );
            info.value = Some((progress as f64 * 100.0).floor());
            info
        });
        response.on_hover_text(format!(
            "{} of {} frames processed",
            number(self.completed),
            number(self.total)
        ));
        ui.painter().rect_filled(rect, 3.0, palette.line);
        if progress > 0.0 {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width() * progress.min(1.0), rect.height()),
                ),
                3.0,
                if self.report.as_ref().is_some_and(|report| report.success()) {
                    palette.success
                } else {
                    palette.accent
                },
            );
        }
        ui.add_space(7.0);
        if self.total > 0 {
            ui.label(
                RichText::new(format!(
                    "{} of {} frames embedded",
                    number(self.successful),
                    number(self.total)
                ))
                .size(12.0),
            );
            if self.processing {
                ui.add(
                    egui::Label::new(RichText::new(&self.current).size(11.0).color(palette.muted))
                        .truncate(),
                )
                .on_hover_text(&self.current);
            }
            ui.add_space(6.0);
            ui.columns(3, |columns| {
                metric(&mut columns[0], "Elapsed", &duration(self.elapsed), palette);
                metric(
                    &mut columns[1],
                    "Speed",
                    &format!("{:.1} fps", self.completed as f64 / self.elapsed.max(0.001)),
                    palette,
                );
                let remaining = if self.processing && self.completed > 0 {
                    duration(
                        self.elapsed / self.completed as f64
                            * self.total.saturating_sub(self.completed) as f64,
                    )
                } else if self.report.is_some() {
                    "0s".into()
                } else {
                    "—".into()
                };
                metric(&mut columns[2], "Remaining", &remaining, palette);
            });
        } else {
            ui.label(
                RichText::new(if self.chosen_frames() > 0 {
                    format!(
                        "{} frames in {} selected sequences",
                        number(self.chosen_frames()),
                        self.enabled.iter().filter(|enabled| **enabled).count()
                    )
                } else {
                    "Scan a folder to prepare your sequences.".into()
                })
                .size(11.0)
                .color(palette.muted),
            );
        }
        if let Some(report) = &self.report {
            let note = if !report.replacement_errors.is_empty() {
                "Review replacement errors in Batch details.".into()
            } else if report.replaced.is_empty() {
                format!("{} frame errors. Originals kept.", report.failures.len())
            } else {
                format!(
                    "{} sequences replaced. Originals moved to Trash.",
                    report.replaced.len()
                )
            };
            ui.add_space(5.0);
            ui.label(RichText::new(note).size(11.0).color(if report.success() {
                palette.muted
            } else {
                palette.warning
            }));
            ui.add_space(5.0);
            ui.horizontal(|ui| {
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
                if ui.button("Save report…").clicked() {
                    self.export_report();
                }
            });
        }
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
            .exact_size(50.0)
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(18, 9)),
            )
            .show(ui, |ui| self.header(ui, palette));
        egui::Panel::left("controls")
            .exact_size(386.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(18, 18)),
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
    ui.label(RichText::new(name).size(13.0).strong().color(palette.text));
    ui.add_space(3.0);
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
    icon_button(
        ui,
        if dark {
            icons::regular::SUN
        } else {
            icons::regular::MOON
        },
        label,
        egui::vec2(30.0, 30.0),
        20.0,
        palette.text,
        true,
    )
}

fn badge(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.3)))
        .corner_radius(4)
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).color(color));
        });
}

fn folder_button(ui: &mut Ui, label: &str, palette: Palette) -> egui::Response {
    icon_button(
        ui,
        icons::regular::FOLDER_OPEN,
        label,
        egui::vec2(28.0, 26.0),
        19.0,
        palette.accent,
        true,
    )
}

fn disclosure_button(ui: &mut Ui, open: bool, label: &str, palette: Palette) -> egui::Response {
    icon_button(
        ui,
        if open {
            icons::regular::CARET_DOWN
        } else {
            icons::regular::CARET_RIGHT
        },
        label,
        egui::vec2(15.0, 22.0),
        12.0,
        palette.muted,
        false,
    )
}

fn icon_button(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    size: egui::Vec2,
    font_size: f32,
    color: Color32,
    frame: bool,
) -> egui::Response {
    let response = ui
        .scope(|ui| {
            // Glyphs use their own font and padding so platform fonts cannot substitute
            // private-use characters or change the compact disclosure column's width.
            ui.spacing_mut().button_padding = egui::Vec2::ZERO;
            let icon = RichText::new(glyph)
                .font(egui::FontId::new(
                    font_size,
                    egui::FontFamily::Name(ICON_FONT.into()),
                ))
                .color(color);
            ui.add(
                egui::Button::new((egui::Atom::grow(), icon, egui::Atom::grow()))
                    .min_size(size)
                    .frame(frame)
                    .corner_radius(4),
            )
        })
        .inner;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_text(label)
}

fn metric(ui: &mut Ui, label: &str, value: &str, palette: Palette) {
    ui.label(RichText::new(label).size(10.0).color(palette.muted));
    ui.label(RichText::new(value).size(12.0));
}

fn number(value: usize) -> String {
    let digits = value.to_string();
    let mut formatted = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

fn set_fonts(ctx: &egui::Context) {
    // Read the platform font in place; do not bundle or redistribute it.
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &["/System/Library/Fonts/SFNS.ttf"]
    } else if cfg!(target_os = "windows") {
        &["C:/Windows/Fonts/segoeui.ttf"]
    } else {
        &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        ]
    };
    let mut fonts = egui::FontDefinitions::default();
    if let Some(data) = candidates.iter().find_map(|path| std::fs::read(path).ok()) {
        fonts
            .font_data
            .insert("system-ui".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system-ui".into());
    }
    fonts.font_data.insert(
        ICON_FONT.into(),
        egui::FontData::from_static(&icons::regular::FONT).into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name(ICON_FONT.into()),
        vec![ICON_FONT.into()],
    );
    ctx.set_fonts(fonts);
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
    visuals.faint_bg_color = if dark {
        Color32::from_rgb(39, 41, 44)
    } else {
        Color32::from_rgb(240, 242, 245)
    };
    visuals.selection.bg_fill = if dark {
        Color32::from_rgb(46, 65, 88)
    } else {
        Color32::from_rgb(220, 233, 250)
    };
    visuals.selection.stroke = Stroke::new(1.0, palette.accent);
    let field = if dark {
        Color32::from_rgb(48, 50, 54)
    } else {
        Color32::WHITE
    };
    let hover = if dark {
        Color32::from_rgb(59, 63, 69)
    } else {
        Color32::from_rgb(233, 238, 246)
    };
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = field;
        widget.weak_bg_fill = field;
        widget.bg_stroke = Stroke::new(1.0, palette.line);
        widget.fg_stroke = Stroke::new(1.0, palette.text);
        widget.corner_radius = egui::CornerRadius::same(4);
        widget.expansion = 0.0;
    }
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.muted);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.muted);
    visuals.text_edit_bg_color = Some(field);
    ctx.set_visuals(visuals);
    ctx.style_mut_of(theme, |style| {
        style.spacing.item_spacing = egui::vec2(7.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        for text in [egui::TextStyle::Body, egui::TextStyle::Button] {
            style
                .text_styles
                .insert(text, egui::FontId::proportional(13.0));
        }
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(11.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(21.0));
    });
}
