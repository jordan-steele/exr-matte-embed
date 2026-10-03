//! Delivery matte embedding with lossless PIZ as the default.
//! Supports one flat scanline part, as exported by After Effects.

use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result, ensure};
use exr::{meta::BlockDescription, prelude::*};

pub mod batch;
pub mod codec;
pub mod sequences;
pub mod settings;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MatteInput {
    pub channel: String,
    pub path: std::path::PathBuf,
}

/// Read a delivery image without silently discarding parts or resolution levels.
fn read_delivery(path: &Path) -> Result<FlatImage> {
    let metadata = MetaData::read_from_file(path, false)
        .with_context(|| format!("Reading header: {}", path.display()))?;
    ensure!(
        metadata.headers.len() == 1,
        "Expected one EXR part: {}",
        path.display()
    );
    let header = &metadata.headers[0];
    ensure!(
        !header.deep,
        "Deep EXRs are unsupported: {}",
        path.display()
    );
    ensure!(
        header.blocks == BlockDescription::ScanLines,
        "Expected a scanline EXR: {}",
        path.display()
    );
    ensure!(
        header
            .channels
            .list
            .iter()
            .all(|channel| channel.sampling == Vec2(1, 1)),
        "Subsampled channels are unsupported: {}",
        path.display()
    );
    read_all_flat_layers_from_file(path)
        .with_context(|| format!("Reading pixels: {}", path.display()))
}

/// Preserve base sample types and attributes; replace only explicitly named mattes.
/// Choosing a lossy output codec can change the encoded pixel values.
/// Matte values come from R and are stored as HALF, matching the Python tool.
/// Publish a complete output atomically and never overwrite an existing file.
pub fn embed_file(
    base_path: &Path,
    mattes: &[MatteInput],
    output_path: &Path,
    compression: Compression,
) -> Result<()> {
    ensure!(!mattes.is_empty(), "At least one matte is required");
    ensure!(
        !output_path.exists(),
        "Output already exists: {}",
        output_path.display()
    );

    let mut names = HashSet::new();
    for matte in mattes {
        ensure!(
            !matte.channel.is_empty()
                && matte.channel.len() <= 255
                && matte.channel.is_ascii()
                && !matte.channel.contains('\0'),
            "Channel names must contain 1–255 ASCII bytes without NUL"
        );
        ensure!(
            !["R", "G", "B", "A"].contains(&matte.channel.as_str()),
            "A matte cannot replace the {} image channel",
            matte.channel
        );
        ensure!(
            names.insert(&matte.channel),
            "Duplicate matte channel: {}",
            matte.channel
        );
    }

    let mut base = read_delivery(base_path)?;
    let layer = &mut base.layer_data[0];
    for matte in mattes {
        let mut image = read_delivery(&matte.path)?;
        let matte_layer = &mut image.layer_data[0];
        ensure!(
            layer.absolute_bounds() == matte_layer.absolute_bounds(),
            "Base and matte data windows differ: {}",
            matte.path.display()
        );
        let red_index = matte_layer
            .channel_data
            .list
            .iter()
            .position(|channel| channel.name.as_slice() == b"R")
            .with_context(|| format!("Matte has no R channel: {}", matte.path.display()))?;
        let red = matte_layer.channel_data.list.remove(red_index);
        let half_samples = match red.sample_data {
            FlatSamples::F16(samples) => samples,
            FlatSamples::F32(samples) => samples.into_iter().map(f16::from_f32).collect(),
            FlatSamples::U32(samples) => samples
                .into_iter()
                .map(|value| f16::from_f32(value as f32))
                .collect(),
        };
        let mut channel = AnyChannel::new(matte.channel.as_str(), FlatSamples::F16(half_samples));
        channel.quantize_linearly = true;
        layer
            .channel_data
            .list
            .retain(|existing| existing.name.as_slice() != matte.channel.as_bytes());
        layer.channel_data.list.push(channel);
    }
    layer.channel_data.list.sort_by(|a, b| a.name.cmp(&b.name));
    layer.encoding.compression = compression;

    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Creating output directory: {}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    base.write()
        .to_unbuffered(temporary.as_file_mut())
        .with_context(|| format!("Writing EXR: {}", output_path.display()))?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(output_path)
        .with_context(|| format!("Publishing EXR: {}", output_path.display()))?;
    Ok(())
}
