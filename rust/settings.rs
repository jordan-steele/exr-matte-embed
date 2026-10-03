use crate::codec::Codec;
use anyhow::{Context, Result};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

pub fn default_workers() -> usize {
    cpu_count().min(4)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub last_folder_path: String,
    pub output_root: String,
    pub custom_output: bool,
    pub compression: Codec,
    pub matte_channel_name: String,
    pub workers: usize,
    pub replace_originals: bool,
    pub dark_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_folder_path: String::new(),
            output_root: String::new(),
            custom_output: false,
            compression: Codec::Piz,
            matte_channel_name: "matte".into(),
            workers: default_workers(),
            replace_originals: false,
            dark_mode: true,
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("com", "EXRTools", "EXRMatteEmbed")
        .map(|dir| dir.config_dir().join("settings.json"))
}

fn legacy_path() -> Option<PathBuf> {
    let dirs = BaseDirs::new()?;
    if cfg!(target_os = "macos") {
        Some(
            dirs.home_dir()
                .join("Library/Application Support/EXRProcessor/config.json"),
        )
    } else if cfg!(target_os = "windows") {
        Some(dirs.config_dir().join("EXRTools/EXRProcessor/config.json"))
    } else {
        Some(dirs.config_dir().join("EXRProcessor/config.json"))
    }
}

pub fn load(path: Option<&Path>) -> (Settings, Option<String>) {
    if path.is_some_and(|path| !path.exists()) {
        return (Settings::default(), None);
    }
    let path = path.map(Path::to_owned).or_else(config_path);
    let candidate = path
        .filter(|path| path.exists())
        .or_else(|| legacy_path().filter(|path| path.exists()));
    let Some(path) = candidate else {
        return (Settings::default(), None);
    };
    match std::fs::read(&path)
        .context("Reading preferences")
        .and_then(|bytes| Ok(serde_json::from_slice::<Settings>(&bytes)?))
    {
        Ok(mut settings) => {
            settings.workers = settings.workers.clamp(1, cpu_count());
            (settings, None)
        }
        Err(error) => (
            Settings::default(),
            Some(format!("Preferences could not be loaded: {error:#}")),
        ),
    }
}

pub fn save(settings: &Settings, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("Preferences need a parent directory")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), settings)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).context("Saving preferences")?;
    Ok(())
}
