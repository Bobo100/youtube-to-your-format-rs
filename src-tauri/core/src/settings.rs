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
    #[serde(deserialize_with = "lenient")]
    pub output_dir: Option<PathBuf>,
    #[serde(deserialize_with = "lenient")]
    pub font_size: FontSize,
    #[serde(deserialize_with = "lenient")]
    pub theme: Theme,
    #[serde(deserialize_with = "lenient")]
    pub language: Language,
    #[serde(deserialize_with = "lenient")]
    pub old_folder_hint_dismissed: bool,
}

/// One unreadable value (say, written by a newer version) resets only that
/// field, not the whole file.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Fields the settings screen changes; absent ones keep their value. Merging
/// on the Rust side under one lock means two quick clicks cannot undo each other.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub font_size: Option<FontSize>,
    pub theme: Option<Theme>,
    pub language: Option<Language>,
    pub old_folder_hint_dismissed: Option<bool>,
    /// `Some("")` resets to the default folder.
    pub output_dir: Option<String>,
}

impl SettingsPatch {
    pub fn apply(&self, settings: &Settings) -> Settings {
        Settings {
            font_size: self.font_size.unwrap_or(settings.font_size),
            theme: self.theme.unwrap_or(settings.theme),
            language: self.language.unwrap_or(settings.language),
            old_folder_hint_dismissed: self.old_folder_hint_dismissed.unwrap_or(settings.old_folder_hint_dismissed),
            output_dir: match &self.output_dir {
                None => settings.output_dir.clone(),
                Some(dir) if dir.is_empty() => None,
                Some(dir) => Some(PathBuf::from(dir)),
            },
        }
    }
}

/// True when a file can be created in `dir` (Program Files, `C:\`, folders
/// under Controlled Folder Access cannot be written).
pub fn can_write(dir: &Path) -> bool {
    tempfile::Builder::new().prefix(".ytf-check").tempfile_in(dir).is_ok()
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("youtube-to-your-format").join("settings.json"))
    }

    /// A missing file means defaults. A damaged one is kept aside as
    /// `settings.broken.json` (for Bobo to look at) and defaults are used. A
    /// file that exists but is briefly locked is retried, not treated as absent
    /// (that would overwrite the real settings on the next save).
    pub fn load(path: &Path) -> Self {
        let Ok(bytes) = crate::tools::retry_io(|| std::fs::read(path)) else {
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
        crate::tools::retry_io(|| std::fs::rename(&tmp, path))
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
    fn an_unknown_value_resets_only_its_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, br#"{"theme":"sepia","language":"en"}"#).unwrap();
        let settings = Settings::load(&path);
        assert_eq!((settings.theme, settings.language), (Theme::Light, Language::En));
    }

    #[test]
    fn patches_change_only_their_fields() {
        let base = Settings { theme: Theme::Dark, output_dir: Some("D:/music".into()), ..Settings::default() };
        let small = SettingsPatch { font_size: Some(FontSize::Large), ..SettingsPatch::default() }.apply(&base);
        assert_eq!((small.font_size, small.theme), (FontSize::Large, Theme::Dark));
        assert_eq!(small.output_dir, base.output_dir);
        let reset = SettingsPatch { output_dir: Some(String::new()), ..SettingsPatch::default() }.apply(&base);
        assert_eq!(reset.output_dir, None);
    }

    #[test]
    fn writable_check() {
        let dir = tempfile::tempdir().unwrap();
        assert!(can_write(dir.path()));
        assert!(!can_write(Path::new(r"Z:\no-such-drive")));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "the probe file is removed");
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
