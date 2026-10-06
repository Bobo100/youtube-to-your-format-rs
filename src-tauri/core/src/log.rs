//! Plain-text log in `%LOCALAPPDATA%\youtube-to-your-format\logs\`. Bobo reads
//! it (via "複製問題資訊") when a family member reports a problem, so every
//! line is self-contained: UTC time + message.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1024 * 1024;
const KEEP: usize = 5;

static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Logs `format!` arguments; before `init` lines only go to stderr.
#[macro_export]
macro_rules! applog {
    ($($arg:tt)*) => {
        $crate::log::write(&format!($($arg)*))
    };
}

pub fn init(dir: PathBuf) {
    let _ = fs::create_dir_all(&dir);
    *DIR.lock().unwrap() = Some(dir);
}

pub fn default_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("youtube-to-your-format").join("logs"))
}

pub fn write(message: &str) {
    eprintln!("{message}");
    let guard = DIR.lock().unwrap();
    let Some(dir) = guard.as_ref() else {
        return;
    };
    let file = dir.join("app.log");
    if fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
        rotate(dir);
    }
    if let Ok(mut out) = OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(out, "{} {}", utc_now(), message.replace('\n', "\n    "));
    }
}

/// The last `lines` lines of the current log.
pub fn tail(lines: usize) -> String {
    let guard = DIR.lock().unwrap();
    let Some(text) = guard.as_ref().and_then(|dir| fs::read_to_string(dir.join("app.log")).ok()) else {
        return String::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn numbered(dir: &Path, n: usize) -> PathBuf {
    dir.join(format!("app.{n}.log"))
}

fn rotate(dir: &Path) {
    // Move the live file aside first: if something holds it open, keep the
    // history instead of shifting (and deleting) numbered files on every write.
    let aside = dir.join("app.rotating.log");
    if fs::rename(dir.join("app.log"), &aside).is_err() {
        return;
    }
    let _ = fs::remove_file(numbered(dir, KEEP - 1));
    for n in (1..KEEP - 1).rev() {
        let _ = fs::rename(numbered(dir, n), numbered(dir, n + 1));
    }
    let _ = fs::rename(aside, numbered(dir, 1));
}

fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    format_utc(secs)
}

/// `YYYY-MM-DD HH:MM:SSZ` (Howard Hinnant's civil-from-days).
fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00Z");
        assert_eq!(format_utc(1_791_307_165), "2026-10-06 17:19:25Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29 00:00:00Z");
    }

    #[test]
    fn rotation_keeps_a_bounded_number_of_files() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..8 {
            fs::write(dir.path().join("app.log"), format!("gen {n}")).unwrap();
            rotate(dir.path());
        }
        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["app.1.log", "app.2.log", "app.3.log", "app.4.log"]);
        assert_eq!(fs::read_to_string(dir.path().join("app.1.log")).unwrap(), "gen 7");
    }
}
