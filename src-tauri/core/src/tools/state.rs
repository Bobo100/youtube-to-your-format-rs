use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Installed tool versions, stored next to the binaries in `bin/state.json`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolState {
    pub ytdlp: Option<String>,
    pub ffmpeg: Option<String>,
    pub deno: Option<String>,
}

impl ToolState {
    /// A missing or corrupt file means "nothing installed": tools get reinstalled.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write-then-rename, so a power cut never leaves a truncated file behind.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&json)?;
        file.sync_all()?;
        drop(file);
        super::install::retry_io(|| std::fs::rename(&tmp, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_file_falls_back_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(ToolState::load(&path), ToolState::default());
    }

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = ToolState {
            ytdlp: Some("2026.08.19".into()),
            ffmpeg: Some("9.0.2".into()),
            deno: None,
        };
        state.save(&path).unwrap();
        assert_eq!(ToolState::load(&path), state);
    }
}
