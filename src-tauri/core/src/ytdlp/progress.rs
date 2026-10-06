//! Parses the machine-readable lines our download arguments ask yt-dlp for.

use serde_json::Value;

pub const PROGRESS_PREFIX: &str = "YTFP ";
pub const PATH_PREFIX: &str = "YTFPATH ";

#[derive(Debug, Clone, PartialEq)]
pub enum LineEvent {
    /// Overall fraction 0.0–1.0 across all streams.
    Progress(f64),
    /// Downloads finished; merging / converting has no progress output.
    Processing,
    FinalPath(String),
}

/// A video download is two streams (video, then audio) that each run 0→100%;
/// this folds them into one bar.
pub struct ProgressTracker {
    streams: usize,
    index: usize,
    current: Option<String>,
}

impl ProgressTracker {
    pub fn new(expected_streams: usize) -> Self {
        Self { streams: expected_streams.max(1), index: 0, current: None }
    }

    pub fn on_line(&mut self, line: &str) -> Option<LineEvent> {
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(path) = line.strip_prefix(PATH_PREFIX) {
            return Some(LineEvent::FinalPath(path.to_owned()));
        }
        let json: Value = serde_json::from_str(line.strip_prefix(PROGRESS_PREFIX)?).ok()?;
        let file = json.get("filename").and_then(Value::as_str).map(str::to_owned);
        if file.is_some() && file != self.current {
            match &self.current {
                Some(_) => self.index = (self.index + 1).min(self.streams - 1),
                // A merged download writes `<base>.f<format>.<ext>` per stream; a
                // single-file fallback (`b`) writes `<base>.<ext>` and is the only stream.
                None if !file.as_deref().is_some_and(is_format_stream) => self.streams = 1,
                None => {}
            }
            self.current = file;
        }
        let number = |key: &str| json.get(key).and_then(Value::as_f64);
        let done = number("downloaded_bytes").unwrap_or(0.0);
        let total = number("total_bytes").or_else(|| number("total_bytes_estimate"));
        let within = total.filter(|t| *t > 0.0).map_or(0.0, |t| (done / t).clamp(0.0, 1.0));
        let finished = json.get("status").and_then(Value::as_str) == Some("finished");
        if finished && self.index + 1 >= self.streams {
            return Some(LineEvent::Processing);
        }
        let overall = (self.index as f64 + if finished { 1.0 } else { within }) / self.streams as f64;
        Some(LineEvent::Progress(overall.min(0.99)))
    }
}

fn is_format_stream(path: &str) -> bool {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _ext)| stem);
    // YouTube format ids are numeric, optionally with a suffix: f133, f251-drc.
    stem.rsplit_once('.').is_some_and(|(_, last)| {
        let id = last.strip_prefix('f').unwrap_or_default();
        id.starts_with(|c: char| c.is_ascii_digit())
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(status: &str, file: &str, done: u64, total: u64) -> String {
        format!(
            r#"{PROGRESS_PREFIX}{{"status": "{status}", "downloaded_bytes": {done}, "total_bytes": {total}, "filename": "C:\\t\\{file}"}}"#
        )
    }

    #[test]
    fn two_streams_make_one_bar() {
        let mut t = ProgressTracker::new(2);
        assert_eq!(t.on_line(&line("downloading", "a.f133.mp4", 50, 100)), Some(LineEvent::Progress(0.25)));
        assert_eq!(t.on_line(&line("finished", "a.f133.mp4", 100, 100)), Some(LineEvent::Progress(0.5)));
        assert_eq!(t.on_line(&line("downloading", "a.f140.m4a", 50, 100)), Some(LineEvent::Progress(0.75)));
        assert_eq!(t.on_line(&line("finished", "a.f140.m4a", 100, 100)), Some(LineEvent::Processing));
    }

    #[test]
    fn single_file_fallback_reaches_full_progress() {
        let mut t = ProgressTracker::new(2);
        assert_eq!(t.on_line(&line("downloading", "a.mp4", 50, 100)), Some(LineEvent::Progress(0.5)));
        assert_eq!(t.on_line(&line("finished", "a.mp4", 100, 100)), Some(LineEvent::Processing));
    }

    #[test]
    fn recognises_per_format_stream_files() {
        assert!(is_format_stream(r"C:\t\測試 100%.f133.mp4"));
        assert!(is_format_stream("C:/t/a.f251-drc.webm"));
        assert!(!is_format_stream(r"C:\t\歌.mp4"));
        assert!(!is_format_stream(r"C:\t\Vol.final.mp4"));
    }

    #[test]
    fn audio_is_one_stream_and_never_reports_100_before_processing() {
        let mut t = ProgressTracker::new(1);
        assert_eq!(t.on_line(&line("downloading", "a.webm", 100, 100)), Some(LineEvent::Progress(0.99)));
        assert_eq!(t.on_line(&line("finished", "a.webm", 100, 100)), Some(LineEvent::Processing));
    }

    #[test]
    fn falls_back_to_estimate_and_ignores_other_lines() {
        let mut t = ProgressTracker::new(1);
        let estimate = format!(
            r#"{PROGRESS_PREFIX}{{"status": "downloading", "downloaded_bytes": 10, "total_bytes": null, "total_bytes_estimate": 40, "filename": "x"}}"#
        );
        assert_eq!(t.on_line(&estimate), Some(LineEvent::Progress(0.25)));
        assert_eq!(t.on_line("[Merger] Merging formats"), None);
        assert_eq!(t.on_line("YTFP not json"), None);
    }

    #[test]
    fn reads_the_final_path() {
        let mut t = ProgressTracker::new(2);
        assert_eq!(
            t.on_line("YTFPATH C:\\Users\\a\\Downloads\\YouTube\\測試 100%.mp4\r\n"),
            Some(LineEvent::FinalPath("C:\\Users\\a\\Downloads\\YouTube\\測試 100%.mp4".into()))
        );
    }
}
