//! The four settings (folder, text size, colours, language), kept in
//! `%LOCALAPPDATA%\youtube-to-your-format\settings.json`.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::folders;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontSize {
    Large,
    /// The mockups were approved at this size.
    #[default]
    Xlarge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Light by default, not following Windows: dark text on light is easier to read.
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Zh,
    En,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `None` = `<Downloads>\YouTube`.
    pub output_dir: Option<PathBuf>,
    pub font_size: FontSize,
    pub theme: Theme,
    pub language: Language,
    pub old_folder_hint_dismissed: bool,
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("youtube-to-your-format").join("settings.json"))
    }

    /// A missing file means defaults. A damaged one is kept aside as
    /// `settings.broken.json` (for Bobo to look at) and defaults are used.
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = std::fs::read(path) else {
            return Self::default();
        };
        match serde_json::from_slice(&bytes) {
            Ok(settings) => settings,
            Err(_) => {
                let _ = std::fs::copy(path, path.with_extension("broken.json"));
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    }

    /// Where downloads go now. A chosen folder that is gone (a USB stick that
    /// was unplugged) falls back to the default instead of failing.
    pub fn effective_output_dir(&self) -> PathBuf {
        self.output_dir
            .clone()
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(folders::default_output_dir)
    }
}

/// The Electron version saved into `<Downloads>\youtube-downloads`.
pub fn old_download_folder() -> Option<PathBuf> {
    let dir = folders::default_output_dir().parent()?.join("youtube-downloads");
    dir.is_dir().then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::load(&dir.path().join("settings.json"));
        assert_eq!(settings, Settings::default());
        assert_eq!((settings.font_size, settings.theme, settings.language), (FontSize::Xlarge, Theme::Light, Language::Zh));
    }

    #[test]
    fn damaged_file_is_kept_aside_and_defaults_are_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{ broken").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert_eq!(std::fs::read(dir.path().join("settings.broken.json")).unwrap(), b"{ broken");
    }

    #[test]
    fn round_trips_and_tolerates_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings { theme: Theme::Dark, language: Language::En, ..Settings::default() };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        std::fs::write(&path, br#"{"theme":"dark"}"#).unwrap();
        assert_eq!(Settings::load(&path).theme, Theme::Dark);
        assert_eq!(Settings::load(&path).font_size, FontSize::Xlarge);
    }

    #[test]
    fn missing_chosen_folder_falls_back_to_default() {
        let gone = Settings { output_dir: Some(PathBuf::from(r"Z:\unplugged\music")), ..Settings::default() };
        assert_eq!(gone.effective_output_dir(), folders::default_output_dir());
        let dir = tempfile::tempdir().unwrap();
        let chosen = Settings { output_dir: Some(dir.path().to_owned()), ..Settings::default() };
        assert_eq!(chosen.effective_output_dir(), dir.path());
    }
}
