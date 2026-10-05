use exr::{meta::attribute::SampleType, prelude::*};
use exr_matte_embed::{
    MatteInput,
    batch::{self, BatchOptions, Plan},
    codec::Codec,
    embed_file,
    sequences::{self, SampleKind},
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

fn image(path: &Path, value: f32) {
    write_rgb_file(path, 8, 4, |_, _| {
        (
            f16::from_f32(value),
            f16::from_f32(value / 2.0),
            f16::from_f32(0.25),
        )
    })
    .unwrap();
    let mut flat = read_all_flat_layers_from_file(path).unwrap();
    flat.layer_data[0].encoding.blocks = Blocks::ScanLines;
    flat.layer_data[0].encoding.compression = Compression::PIZ;
    flat.layer_data[0].encoding.line_order = LineOrder::Increasing;
    flat.write().to_file(path).unwrap();
}

fn fixtures(root: &Path) {
    for folder in ["SHOT", "SHOT_matte", "SHOT_matteHero", "SHOT_matteR"] {
        std::fs::create_dir(root.join(folder)).unwrap();
        for frame in [1000, 1001] {
            image(
                &root.join(folder).join(format!("{folder}.{frame:07}.exr")),
                0.5,
            );
        }
    }
}

fn options() -> BatchOptions {
    BatchOptions {
        compression: Codec::Piz,
        matte_channel: "DI_Matte".into(),
        workers: exr_matte_embed::settings::cpu_count().min(2),
        output_root: None,
        replace_originals: false,
    }
}

#[test]
fn semantic_channels_numeric_pairing_and_existing_outputs() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    let scan = sequences::scan(root.path()).unwrap();
    assert_eq!(scan.sequences.len(), 1);
    let sequence = &scan.sequences[0];
    assert!(sequence.ready());
    assert_eq!(
        sequence.channel_names("DI_Matte").unwrap(),
        ["DI_Matte", "DI_Matte.hero", "DI_Matte.matte_r"]
    );
    let plan = Plan::new(scan.sequences.clone(), options()).unwrap();
    assert_eq!(plan.jobs.len(), 2);
    std::fs::create_dir(sequence.output_folder(None)).unwrap();
    std::fs::write(&plan.jobs[0].request.output, "keep this output").unwrap();
    assert!(Plan::new(scan.sequences, options()).is_err());
    assert_eq!(
        std::fs::read_to_string(&plan.jobs[0].request.output).unwrap(),
        "keep this output"
    );
}

#[test]
fn equal_counts_with_different_frame_numbers_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    std::fs::rename(
        root.path().join("SHOT_matte/SHOT_matte.0001001.exr"),
        root.path().join("SHOT_matte/SHOT_matte.1002.exr"),
    )
    .unwrap();
    let scan = sequences::scan(root.path()).unwrap();
    assert!(!scan.sequences[0].ready());
    assert!(
        scan.sequences[0]
            .issues
            .iter()
            .any(|issue| issue.contains("frame numbers differ"))
    );
    assert!(Plan::new(scan.sequences, options()).is_err());
}

#[test]
fn duplicate_frame_and_resolved_channel_collisions_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    image(&root.path().join("SHOT/SHOT.1000.exr"), 1.0);
    assert!(!sequences::scan(root.path()).unwrap().sequences[0].ready());
    std::fs::remove_file(root.path().join("SHOT/SHOT.1000.exr")).unwrap();
    let extra = root.path().join("SHOT_mattematte_r");
    std::fs::create_dir(&extra).unwrap();
    for frame in [1000, 1001] {
        image(&extra.join(format!("extra.{frame}.exr")), 0.25);
    }
    assert!(!sequences::scan(root.path()).unwrap().sequences[0].ready());
}

#[test]
fn separators_after_matte_do_not_reach_channel_names() {
    for (suffix, expected) in [
        ("", "DI_Matte"),
        ("_", "DI_Matte"),
        ("hero", "DI_Matte.hero"),
        ("_hero", "DI_Matte.hero"),
        ("-hero", "DI_Matte.hero"),
        (".hero", "DI_Matte.hero"),
        (" hero_", "DI_Matte.hero"),
        ("_hero_left", "DI_Matte.hero_left"),
        ("_r", "DI_Matte.matte_r"),
    ] {
        assert_eq!(
            sequences::channel_name("DI_Matte", suffix),
            expected,
            "{suffix:?}"
        );
    }

    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    let separated = root.path().join("SHOT_matte_fx");
    std::fs::create_dir(&separated).unwrap();
    for frame in [1000, 1001] {
        image(&separated.join(format!("fx.{frame}.exr")), 0.25);
    }
    let scan = sequences::scan(root.path()).unwrap();
    assert!(scan.sequences[0].ready(), "{:?}", scan.sequences[0].issues);
    assert_eq!(
        scan.sequences[0].channel_names("matte").unwrap(),
        ["matte", "matte.hero", "matte.matte_r", "matte.fx"]
    );

    // Folders that differ only by a separator resolve to the same channel.
    let duplicate = root.path().join("SHOT_matte_hero");
    std::fs::create_dir(&duplicate).unwrap();
    for frame in [1000, 1001] {
        image(&duplicate.join(format!("hero.{frame}.exr")), 0.25);
    }
    let scan = sequences::scan(root.path()).unwrap();
    assert!(!scan.sequences[0].ready());
    assert!(
        scan.sequences[0]
            .issues
            .iter()
            .any(|issue| issue.contains("same channel"))
    );
}

#[test]
fn persistent_worker_pipeline_and_cancellation() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    let plan = Plan::new(sequences::scan(root.path()).unwrap().sequences, options()).unwrap();
    let (tx, _rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(true));
    let report = batch::run(
        plan.clone(),
        Path::new(env!("CARGO_BIN_EXE_exr-matte-embed-cli")),
        stop,
        tx,
    );
    assert!(report.cancelled);
    assert_eq!(report.completed, 0);
    assert!(!plan.jobs[0].request.output.exists());
    let (tx, _rx) = mpsc::channel();
    let report = batch::run(
        plan.clone(),
        Path::new(env!("CARGO_BIN_EXE_exr-matte-embed-cli")),
        Arc::new(AtomicBool::new(false)),
        tx,
    );
    assert!(report.success());
    assert_eq!(report.successful, 2);
    for job in &plan.jobs {
        let output = read_all_flat_layers_from_file(&job.request.output).unwrap();
        let names: Vec<_> = output.layer_data[0]
            .channel_data
            .list
            .iter()
            .map(|channel| channel.name.to_string())
            .collect();
        assert_eq!(
            names,
            [
                "B",
                "DI_Matte",
                "DI_Matte.hero",
                "DI_Matte.matte_r",
                "G",
                "R"
            ]
        );
    }
}

#[test]
fn failed_trash_keeps_original_backup_and_published_outputs() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    let sequence = sequences::scan(root.path()).unwrap().sequences.remove(0);
    let output = sequence.output_folder(None);
    std::fs::create_dir(&output).unwrap();
    for file in sequence.files.values() {
        std::fs::write(output.join(file.file_name().unwrap()), "embedded").unwrap();
    }
    let error =
        batch::replace_sequence(&sequence, &|_| anyhow::bail!("Trash unavailable")).unwrap_err();
    assert!(format!("{error:#}").contains("Original backup remains"));
    let backup = std::fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".exr-original-")
        })
        .unwrap();
    assert!(backup.join("SHOT/SHOT.0001000.exr").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("SHOT/SHOT.0001000.exr")).unwrap(),
        "embedded"
    );
    assert!(root.path().join("SHOT_matte").exists());
}

#[test]
fn incomplete_replacement_leaves_sources_in_place() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    let sequence = sequences::scan(root.path()).unwrap().sequences.remove(0);
    assert!(batch::replace_sequence(&sequence, &|_| Ok(())).is_err());
    assert!(root.path().join("SHOT/SHOT.0001000.exr").exists());
}

#[test]
fn frame_matching_accepts_padding_case_and_unicode_names() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    std::fs::rename(
        root.path().join("SHOT/SHOT.0001000.exr"),
        root.path().join("SHOT/Shot_1000.EXR"),
    )
    .unwrap();
    std::fs::rename(
        root.path().join("SHOT_matte/SHOT_matte.0001000.exr"),
        root.path().join("SHOT_matte/é1000.exr"),
    )
    .unwrap();
    let scan = sequences::scan(root.path()).unwrap();
    assert!(scan.sequences[0].ready());
    assert_eq!(Plan::new(scan.sequences, options()).unwrap().jobs.len(), 2);
}

#[test]
fn matte_missing_red_or_offset_window_fails_scan_before_processing() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    let matte = root.path().join("SHOT_matte/SHOT_matte.0001000.exr");
    let mut flat = read_all_flat_layers_from_file(&matte).unwrap();
    flat.layer_data[0]
        .channel_data
        .list
        .retain(|channel| channel.name.as_slice() != b"R");
    flat.layer_data[0].attributes.layer_position = Vec2(1, 0);
    flat.write().to_file(&matte).unwrap();
    let scan = sequences::scan(root.path()).unwrap();
    assert!(!scan.sequences[0].ready());
    assert!(
        scan.sequences[0]
            .issues
            .iter()
            .any(|issue| issue.contains("no R channel"))
    );
    assert!(
        scan.sequences[0]
            .issues
            .iter()
            .any(|issue| issue.contains("data windows differ"))
    );
    assert!(Plan::new(scan.sequences, options()).is_err());
}

#[test]
fn cancelling_in_flight_keeps_completed_outputs_and_originals() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    for folder in ["SHOT", "SHOT_matte", "SHOT_matteHero", "SHOT_matteR"] {
        for number in 1002..1128 {
            std::fs::copy(
                root.path()
                    .join(folder)
                    .join(format!("{folder}.0001000.exr")),
                root.path()
                    .join(folder)
                    .join(format!("{folder}.{number}.exr")),
            )
            .unwrap();
        }
    }
    let mut options = options();
    options.workers = 1;
    options.replace_originals = true;
    let plan = Plan::new(sequences::scan(root.path()).unwrap().sequences, options).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let child_stop = stop.clone();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        batch::run(
            plan,
            Path::new(env!("CARGO_BIN_EXE_exr-matte-embed-cli")),
            child_stop,
            tx,
        )
    });
    loop {
        if matches!(rx.recv().unwrap(), batch::BatchEvent::Progress(_)) {
            break;
        }
    }
    stop.store(true, Ordering::Release);
    let report = handle.join().unwrap();
    assert!(report.cancelled);
    assert!(report.successful > 0 && report.successful < report.total);
    assert_eq!(report.successful, report.completed);
    assert!(report.replaced.is_empty());
    assert!(root.path().join("SHOT/SHOT.0001000.exr").exists());
    assert!(
        root.path()
            .join("SHOT_matte/SHOT_matte.0001000.exr")
            .exists()
    );
    let outputs: Vec<_> = std::fs::read_dir(root.path().join("SHOT_embedded"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(outputs.len(), report.successful);
    for file in outputs {
        assert!(read_all_flat_layers_from_file(file).is_ok());
    }
}

#[test]
fn a_later_corrupt_frame_prevents_all_replacement() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    std::fs::write(
        root.path().join("SHOT/SHOT.0001001.exr"),
        b"incomplete source",
    )
    .unwrap();
    let mut options = options();
    options.replace_originals = true;
    let plan = Plan::new(sequences::scan(root.path()).unwrap().sequences, options).unwrap();
    let (tx, _rx) = mpsc::channel();
    let report = batch::run(
        plan,
        Path::new(env!("CARGO_BIN_EXE_exr-matte-embed-cli")),
        Arc::new(AtomicBool::new(false)),
        tx,
    );
    assert!(!report.success());
    assert_eq!(report.successful, 1);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].frame, 1001);
    assert!(report.replaced.is_empty());
    assert!(root.path().join("SHOT/SHOT.0001000.exr").exists());
    assert!(
        root.path()
            .join("SHOT_matte/SHOT_matte.0001000.exr")
            .exists()
    );
}

const WIDTH: usize = 9;
const HEIGHT: usize = 7;

/// Values that HALF cannot represent, so any conversion is detectable.
fn float_samples(offset: f32) -> Vec<f32> {
    (0..WIDTH * HEIGHT)
        .map(|index| offset + index as f32 * 0.012_345_679)
        .collect()
}

fn write_layer(path: &Path, channels: Vec<AnyChannel<FlatSamples>>) {
    let layer = Layer::new(
        (WIDTH, HEIGHT),
        LayerAttributes::default(),
        Encoding {
            compression: Compression::PIZ,
            blocks: Blocks::ScanLines,
            line_order: LineOrder::Increasing,
        },
        AnyChannels::sort(channels.into_iter().collect()),
    );
    Image::from_layer(layer).write().to_file(path).unwrap();
}

fn channel(name: &str, samples: FlatSamples, linear: bool) -> AnyChannel<FlatSamples> {
    AnyChannel {
        quantize_linearly: linear,
        ..AnyChannel::new(name, samples)
    }
}

fn bits(samples: &FlatSamples) -> (SampleType, Vec<u32>) {
    match samples {
        FlatSamples::F16(values) => (
            SampleType::F16,
            values.iter().map(|value| value.to_bits().into()).collect(),
        ),
        FlatSamples::F32(values) => (
            SampleType::F32,
            values.iter().map(|value| value.to_bits()).collect(),
        ),
        FlatSamples::U32(values) => (SampleType::U32, values.clone()),
    }
}

fn find<'a>(image: &'a FlatImage, name: &str) -> &'a AnyChannel<FlatSamples> {
    image.layer_data[0]
        .channel_data
        .list
        .iter()
        .find(|channel| channel.name.to_string() == name)
        .unwrap_or_else(|| panic!("missing channel {name}"))
}

/// Python 1.x read and wrote every channel as HALF. Mattes and base channels
/// must now keep their source sample type, bits and pLinear flag exactly.
#[test]
fn mattes_and_base_channels_keep_source_sample_types() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("base.exr");
    write_layer(
        &base,
        vec![
            channel("R", FlatSamples::F32(float_samples(-2.0)), false),
            channel(
                "G",
                FlatSamples::F16(float_samples(0.0).into_iter().map(f16::from_f32).collect()),
                false,
            ),
            channel(
                "B",
                FlatSamples::U32((0..WIDTH * HEIGHT).map(|i| i as u32 * 7_919).collect()),
                false,
            ),
            channel("A", FlatSamples::F32(float_samples(0.5)), true),
        ],
    );
    let mattes = [
        ("matte", FlatSamples::F32(float_samples(0.25)), false),
        (
            "matte.half",
            FlatSamples::F16(float_samples(0.75).into_iter().map(f16::from_f32).collect()),
            true,
        ),
        (
            "matte.id",
            FlatSamples::U32((0..WIDTH * HEIGHT).map(|i| u32::MAX - i as u32).collect()),
            false,
        ),
    ];
    let mut inputs = Vec::new();
    for (name, samples, linear) in &mattes {
        let path = root.path().join(format!("{name}.exr"));
        write_layer(
            &path,
            vec![
                channel("R", samples.clone(), *linear),
                channel("G", FlatSamples::F32(float_samples(9.0)), false),
            ],
        );
        inputs.push(MatteInput {
            channel: (*name).into(),
            path,
        });
    }
    for codec in [Codec::None, Codec::Rle, Codec::Zip, Codec::Zips, Codec::Piz] {
        let output = root.path().join(format!("out-{}.exr", codec.name()));
        embed_file(&base, &inputs, &output, codec.into()).unwrap();
        let written = read_all_flat_layers_from_file(&output).unwrap();
        let source = read_all_flat_layers_from_file(&base).unwrap();
        for original in &source.layer_data[0].channel_data.list {
            let kept = find(&written, &original.name.to_string());
            assert_eq!(bits(&kept.sample_data), bits(&original.sample_data));
            assert_eq!(kept.quantize_linearly, original.quantize_linearly);
        }
        for (name, samples, linear) in &mattes {
            let embedded = find(&written, name);
            assert_eq!(
                bits(&embedded.sample_data),
                bits(samples),
                "{name} with {codec:?}"
            );
            assert_eq!(embedded.quantize_linearly, *linear, "{name} pLinear");
        }
    }
}

#[test]
fn scan_reports_source_sample_types_and_can_be_cancelled() {
    let root = tempfile::tempdir().unwrap();
    for folder in ["SHOT", "SHOT_matte", "SHOT_matteHero"] {
        std::fs::create_dir(root.path().join(folder)).unwrap();
    }
    let half = || FlatSamples::F16(vec![f16::ONE; WIDTH * HEIGHT]);
    for frame in [1000, 1001] {
        write_layer(
            &root.path().join(format!("SHOT/SHOT.{frame}.exr")),
            ["B", "G", "R"]
                .map(|name| channel(name, half(), false))
                .into(),
        );
        write_layer(
            &root.path().join(format!("SHOT_matte/m.{frame}.exr")),
            vec![channel("R", half(), false)],
        );
        write_layer(
            &root.path().join(format!("SHOT_matteHero/h.{frame}.exr")),
            vec![channel("R", FlatSamples::F32(float_samples(0.0)), false)],
        );
    }
    let scan = sequences::scan(root.path()).unwrap();
    let sequence = &scan.sequences[0];
    assert!(sequence.ready(), "{:?}", sequence.issues);
    assert_eq!(
        sequence.image.as_ref().unwrap().uniform_sample_type(),
        Some(SampleKind::Half)
    );
    let kinds: Vec<_> = sequence.mattes.iter().map(|m| m.sample_type()).collect();
    assert_eq!(kinds, [Some(SampleKind::Half), Some(SampleKind::Float)]);

    let error = sequences::scan_until(root.path(), &AtomicBool::new(true)).unwrap_err();
    assert!(error.to_string().contains("cancelled"));
}

#[test]
fn successful_replacement_publishes_outputs_and_retains_originals_in_trash() {
    let root = tempfile::tempdir().unwrap();
    fixtures(root.path());
    let sequence = sequences::scan(root.path()).unwrap().sequences.remove(0);
    let plan = Plan::new(vec![sequence.clone()], options()).unwrap();
    let (tx, _rx) = mpsc::channel();
    let report = batch::run(
        plan,
        Path::new(env!("CARGO_BIN_EXE_exr-matte-embed-cli")),
        Arc::new(AtomicBool::new(false)),
        tx,
    );
    assert!(report.success());
    let simulated_trash = root.path().join("trash");
    std::fs::create_dir(&simulated_trash).unwrap();
    batch::replace_sequence(&sequence, &|path| {
        std::fs::rename(path, simulated_trash.join(path.file_name().unwrap()))?;
        Ok(())
    })
    .unwrap();
    assert!(!sequence.output_folder(None).exists());
    assert!(simulated_trash.join("SHOT/SHOT.0001000.exr").exists());
    for matte in &sequence.mattes {
        assert!(!matte.folder.exists());
        assert!(
            simulated_trash
                .join(matte.folder.file_name().unwrap())
                .exists()
        );
    }
    for file in sequence.files.values() {
        let published = read_all_flat_layers_from_file(file).unwrap();
        assert_eq!(published.layer_data[0].channel_data.list.len(), 6);
    }
}
