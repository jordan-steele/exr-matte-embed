use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, bail, ensure};
use exr::{
    meta::{BlockDescription, attribute::SampleType},
    prelude::{MetaData, Vec2},
};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SampleKind {
    Half,
    Float,
    Uint,
}

impl SampleKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Half => "half",
            Self::Float => "float",
            Self::Uint => "uint",
        }
    }
}

impl From<SampleType> for SampleKind {
    fn from(sample_type: SampleType) -> Self {
        match sample_type {
            SampleType::F16 => Self::Half,
            SampleType::F32 => Self::Float,
            SampleType::U32 => Self::Uint,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    pub name: String,
    pub sample_type: SampleKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub width: usize,
    pub height: usize,
    pub origin: [i32; 2],
    pub channels: Vec<ChannelInfo>,
}

impl ImageInfo {
    pub fn sample_type(&self, channel: &str) -> Option<SampleKind> {
        self.channels
            .iter()
            .find(|info| info.name == channel)
            .map(|info| info.sample_type)
    }

    /// The shared sample type, or `None` when channels mix types.
    pub fn uniform_sample_type(&self) -> Option<SampleKind> {
        let first = self.channels.first()?.sample_type;
        self.channels
            .iter()
            .all(|info| info.sample_type == first)
            .then_some(first)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatteSequence {
    pub suffix: String,
    pub folder: PathBuf,
    pub files: BTreeMap<u64, PathBuf>,
    /// Header of the first frame; embedded mattes keep its R sample type.
    pub image: Option<ImageInfo>,
}

impl MatteSequence {
    pub fn sample_type(&self) -> Option<SampleKind> {
        self.image.as_ref()?.sample_type("R")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    pub folder: PathBuf,
    pub files: BTreeMap<u64, PathBuf>,
    pub mattes: Vec<MatteSequence>,
    pub image: Option<ImageInfo>,
    pub issues: Vec<String>,
    pub existing_outputs: usize,
}

impl Sequence {
    pub fn name(&self) -> String {
        self.folder
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }

    pub fn output_folder(&self, output_root: Option<&Path>) -> PathBuf {
        match output_root {
            Some(root) => root.join(format!("{}_embedded", self.name())),
            None => self
                .folder
                .with_file_name(format!("{}_embedded", self.name())),
        }
    }

    pub fn ready(&self) -> bool {
        self.issues.is_empty() && !self.files.is_empty()
    }

    pub fn channel_names(&self, prefix: &str) -> Result<Vec<String>> {
        validate_prefix(prefix)?;
        let names: Vec<_> = self
            .mattes
            .iter()
            .map(|matte| channel_name(prefix, &matte.suffix))
            .collect();
        let unique: BTreeSet<_> = names.iter().collect();
        ensure!(
            unique.len() == names.len(),
            "Two matte folders map to the same channel in {}",
            self.name()
        );
        for name in &names {
            validate_channel(name)?;
        }
        Ok(names)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub root: PathBuf,
    pub sequences: Vec<Sequence>,
    pub warnings: Vec<String>,
}

pub fn validate_channel(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= 255 && name.is_ascii() && !name.contains('\0'),
        "Channel names require 1–255 ASCII characters without NUL"
    );
    ensure!(
        !["R", "G", "B", "A"].contains(&name),
        "A matte cannot replace the {name} image channel"
    );
    Ok(())
}

pub fn validate_prefix(prefix: &str) -> Result<()> {
    validate_channel(prefix)?;
    ensure!(
        prefix.trim() == prefix && !prefix.chars().any(char::is_control),
        "Remove whitespace at the ends and control characters from the matte channel name"
    );
    Ok(())
}

/// Separators around a folder suffix are dropped, so `SHOT_matte_hero`,
/// `SHOT_matte-hero` and `SHOT_matteHero` all become `{prefix}.hero`.
/// Python 1.x kept them, producing names such as `matte._hero`.
pub fn channel_name(prefix: &str, suffix: &str) -> String {
    let suffix = suffix.trim_matches(['_', '-', '.', ' ']);
    if suffix.is_empty() {
        prefix.to_owned()
    } else if ["r", "g", "b", "a"].contains(&suffix) {
        format!("{prefix}.matte_{suffix}")
    } else {
        format!("{prefix}.{suffix}")
    }
}

pub fn frame_files(folder: &Path) -> Result<BTreeMap<u64, PathBuf>> {
    let mut files = BTreeMap::new();
    for entry in
        std::fs::read_dir(folder).with_context(|| format!("Reading {}", folder.display()))?
    {
        let path = entry?.path();
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exr"))
        {
            continue;
        }
        let stem = path
            .file_stem()
            .context("Missing filename")?
            .to_string_lossy();
        let digits_start = stem
            .bytes()
            .rposition(|c| !c.is_ascii_digit())
            .map_or(0, |index| index + 1);
        let digits = &stem[digits_start..];
        ensure!(
            !digits.is_empty(),
            "Use numbered EXRs such as SHOT.1000.exr: {}",
            path.display()
        );
        let number: u64 = digits.parse().context("Frame number is too large")?;
        ensure!(
            files.insert(number, path.clone()).is_none(),
            "Duplicate frame number {number} in {}",
            folder.display()
        );
    }
    ensure!(
        !files.is_empty(),
        "No numbered EXRs in {}",
        folder.display()
    );
    Ok(files)
}

pub fn inspect_image(path: &Path) -> Result<ImageInfo> {
    let metadata = MetaData::read_from_file(path, false)
        .with_context(|| format!("Reading {}", path.display()))?;
    ensure!(
        metadata.headers.len() == 1,
        "Expected one EXR part: {}",
        path.display()
    );
    let header = &metadata.headers[0];
    ensure!(
        !header.deep && header.blocks == BlockDescription::ScanLines,
        "Use flat scanline EXRs: {}",
        path.display()
    );
    ensure!(
        header
            .channels
            .list
            .iter()
            .all(|c| c.sampling == Vec2(1, 1)),
        "Subsampled EXRs are unsupported: {}",
        path.display()
    );
    Ok(ImageInfo {
        width: header.layer_size.width(),
        height: header.layer_size.height(),
        origin: [
            header.own_attributes.layer_position.x(),
            header.own_attributes.layer_position.y(),
        ],
        channels: header
            .channels
            .list
            .iter()
            .map(|c| ChannelInfo {
                name: c.name.to_string(),
                sample_type: c.sample_type.into(),
            })
            .collect(),
    })
}

pub fn scan(root: &Path) -> Result<ScanResult> {
    scan_until(root, &AtomicBool::new(false))
}

/// Scan, returning an error early once `stop` is set.
pub fn scan_until(root: &Path, stop: &AtomicBool) -> Result<ScanResult> {
    ensure!(root.is_dir(), "Folder does not exist: {}", root.display());
    let root = root.canonicalize()?;
    let mut result = ScanResult {
        root: root.clone(),
        ..Default::default()
    };
    let mut grouped = BTreeMap::<PathBuf, Vec<(String, PathBuf)>>::new();
    for entry in WalkDir::new(&root).follow_links(false).sort_by_file_name() {
        ensure!(!stop.load(Ordering::Relaxed), "Scan cancelled");
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                result.warnings.push(error.to_string());
                continue;
            }
        };
        if !entry.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let Some((base, suffix)) = name.rsplit_once("_matte") else {
            continue;
        };
        if base.is_empty() {
            continue;
        }
        let Some(parent) = entry.path().parent() else {
            continue;
        };
        let folder = parent.join(base);
        if !folder.is_dir() {
            result.warnings.push(format!(
                "Missing source folder for {}",
                entry.path().display()
            ));
            continue;
        }
        grouped
            .entry(folder)
            .or_default()
            .push((suffix.to_lowercase(), entry.path().to_owned()));
    }
    for (folder, matte_folders) in grouped {
        ensure!(!stop.load(Ordering::Relaxed), "Scan cancelled");
        let mut sequence = Sequence {
            folder: folder.clone(),
            files: BTreeMap::new(),
            mattes: Vec::new(),
            image: None,
            issues: Vec::new(),
            existing_outputs: 0,
        };
        match frame_files(&folder) {
            Ok(files) => sequence.files = files,
            Err(error) => sequence.issues.push(format!("{error:#}")),
        }
        for (suffix, matte_folder) in matte_folders {
            match frame_files(&matte_folder) {
                Ok(files) => {
                    if files.keys().ne(sequence.files.keys()) {
                        let missing: Vec<_> = sequence
                            .files
                            .keys()
                            .filter(|n| !files.contains_key(n))
                            .take(8)
                            .copied()
                            .collect();
                        let extra: Vec<_> = files
                            .keys()
                            .filter(|n| !sequence.files.contains_key(n))
                            .take(8)
                            .copied()
                            .collect();
                        sequence.issues.push(format!(
                            "{}: frame numbers differ (missing {missing:?}, extra {extra:?})",
                            matte_folder
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                        ));
                    }
                    sequence.mattes.push(MatteSequence {
                        suffix,
                        folder: matte_folder,
                        files,
                        image: None,
                    });
                }
                Err(error) => sequence.issues.push(format!("{error:#}")),
            }
        }
        if let Err(error) = sequence.channel_names("matte") {
            sequence.issues.push(format!("{error:#}"));
        }
        if let Some(first) = sequence.files.values().next() {
            match inspect_image(first) {
                Ok(image) => sequence.image = Some(image),
                Err(error) => sequence.issues.push(format!("{error:#}")),
            }
        }
        for matte in &mut sequence.mattes {
            if let Some(first) = matte.files.values().next() {
                match inspect_image(first) {
                    Ok(info) => {
                        if info.sample_type("R").is_none() {
                            sequence
                                .issues
                                .push(format!("Matte has no R channel: {}", first.display()));
                        }
                        if let Some(base) = &sequence.image
                            && (base.width, base.height, base.origin)
                                != (info.width, info.height, info.origin)
                        {
                            sequence.issues.push(format!(
                                "Base and matte data windows differ: {}",
                                first.display()
                            ));
                        }
                        matte.image = Some(info);
                    }
                    Err(error) => sequence.issues.push(format!("{error:#}")),
                }
            }
        }
        // Skip per-frame checks when no output folder exists; they are slow on network volumes.
        let output = sequence.output_folder(None);
        if output.exists() {
            sequence.existing_outputs = sequence
                .files
                .values()
                .filter(|file| {
                    file.file_name()
                        .is_some_and(|name| output.join(name).exists())
                })
                .count();
        }
        result.sequences.push(sequence);
    }
    Ok(result)
}

pub fn require_unique_destinations(
    sequences: &[Sequence],
    output_root: Option<&Path>,
) -> Result<()> {
    let mut destinations = BTreeSet::new();
    for sequence in sequences {
        if !destinations.insert(sequence.output_folder(output_root)) {
            bail!(
                "Two source folders share the destination name {}; choose separate batches or output roots",
                sequence.name()
            );
        }
    }
    Ok(())
}
