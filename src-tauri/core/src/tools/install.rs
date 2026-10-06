use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub fn new_path(target: &Path) -> PathBuf {
    with_suffix(target, ".new")
}

pub fn old_path(target: &Path) -> PathBuf {
    with_suffix(target, ".old")
}

/// Antivirus scans freshly written files and briefly holds them open, so
/// renames and deletes retry on sharing violations / access denied.
pub fn retry_io<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    const SHARING_VIOLATION: i32 = 32;
    let mut delay = Duration::from_millis(200);
    for _ in 0..5 {
        match op() {
            Err(e)
                if e.kind() == io::ErrorKind::PermissionDenied
                    || e.raw_os_error() == Some(SHARING_VIOLATION) =>
            {
                std::thread::sleep(delay);
                delay *= 2;
            }
            other => return other,
        }
    }
    op()
}

/// Puts `target.new` in place of `target`, keeping the previous file as
/// `target.old` for rollback. Windows cannot overwrite a running exe, so the
/// caller must make sure nothing is executing `target`.
pub fn replace_with_new(target: &Path) -> io::Result<()> {
    let new = new_path(target);
    OpenOptions::new().write(true).open(&new)?.sync_all()?;
    if target.exists() {
        let old = old_path(target);
        if old.exists() {
            retry_io(|| fs::remove_file(&old))?;
        }
        retry_io(|| fs::rename(target, &old))?;
    }
    retry_io(|| fs::rename(&new, target))
}

/// Undoes `replace_with_new` after the new file failed verification: the
/// previous version comes back, or a broken first install is removed.
pub fn rollback(target: &Path) -> io::Result<()> {
    let old = old_path(target);
    if target.exists() {
        retry_io(|| fs::remove_file(target))?;
    }
    if old.exists() {
        retry_io(|| fs::rename(&old, target))?;
    }
    Ok(())
}

/// Writes each `wanted` file name found in the archive (at any depth) to
/// `<dir>/<name>.new`. Returns how many distinct names were found.
pub fn extract_zip(archive: &Path, wanted: &[&str], dir: &Path) -> io::Result<usize> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(io::Error::other)?;
    let mut found = HashSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(io::Error::other)?;
        let Some(name) = matching(entry.name(), wanted) else {
            continue;
        };
        if found.insert(name) {
            write_new(&mut entry, &dir.join(name))?;
        }
    }
    Ok(found.len())
}

pub fn extract_7z(archive: &Path, wanted: &[&str], dir: &Path) -> io::Result<usize> {
    let mut reader =
        sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty())
            .map_err(io::Error::other)?;
    let mut found = HashSet::new();
    reader
        .for_each_entries(|entry, data| {
            // Solid archive: every entry must be read through, even unwanted ones.
            match matching(entry.name(), wanted) {
                Some(name) if !entry.is_directory() && found.insert(name) => {
                    write_new(data, &dir.join(name))?;
                }
                _ => {
                    io::copy(data, &mut io::sink())?;
                }
            }
            Ok(true)
        })
        .map_err(io::Error::other)?;
    Ok(found.len())
}

fn matching<'a>(entry_name: &str, wanted: &[&'a str]) -> Option<&'a str> {
    let normalized = entry_name.replace('\\', "/");
    wanted
        .iter()
        .copied()
        .find(|w| normalized == *w || normalized.ends_with(&format!("/{w}")))
}

fn write_new(data: &mut dyn Read, target: &Path) -> io::Result<()> {
    let mut out = File::create(new_path(target))?;
    io::copy(data, &mut out)?;
    out.flush()?;
    out.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_with(archive: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(File::create(archive).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn replace_keeps_previous_as_old() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("yt-dlp.exe");
        fs::write(&target, b"v1").unwrap();
        fs::write(new_path(&target), b"v2").unwrap();
        replace_with_new(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v2");
        assert_eq!(fs::read(old_path(&target)).unwrap(), b"v1");
        assert!(!new_path(&target).exists());

        fs::write(new_path(&target), b"v3").unwrap();
        replace_with_new(&target).unwrap();
        assert_eq!(fs::read(old_path(&target)).unwrap(), b"v2");
    }

    #[test]
    fn replace_works_on_first_install() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deno.exe");
        fs::write(new_path(&target), b"v1").unwrap();
        replace_with_new(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v1");
        assert!(!old_path(&target).exists());
    }

    #[test]
    fn rollback_restores_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("yt-dlp.exe");
        fs::write(&target, b"good").unwrap();
        fs::write(new_path(&target), b"broken").unwrap();
        replace_with_new(&target).unwrap();
        rollback(&target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"good");
        assert!(!old_path(&target).exists());
    }

    #[test]
    fn rollback_of_first_install_removes_the_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("deno.exe");
        fs::write(new_path(&target), b"broken").unwrap();
        replace_with_new(&target).unwrap();
        rollback(&target).unwrap();
        assert!(!target.exists());
    }

    #[test]
    fn matches_nested_entries_only_on_path_boundary() {
        let wanted = ["ffmpeg.exe", "ffprobe.exe"];
        assert_eq!(
            matching("ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe", &wanted),
            Some("ffmpeg.exe")
        );
        assert_eq!(matching("deno.exe", &["deno.exe"]), Some("deno.exe"));
        assert_eq!(matching("bin/notffmpeg.exe", &wanted), None);
    }

    #[test]
    fn extracts_wanted_zip_entries_as_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        zip_with(&archive, &[("deno.exe", b"deno"), ("README.md", b"x")]);
        assert_eq!(extract_zip(&archive, &["deno.exe"], dir.path()).unwrap(), 1);
        assert_eq!(fs::read(new_path(&dir.path().join("deno.exe"))).unwrap(), b"deno");
    }

    #[test]
    fn duplicate_entries_count_once() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        zip_with(&archive, &[("a/deno.exe", b"first"), ("b/deno.exe", b"second")]);
        assert_eq!(extract_zip(&archive, &["deno.exe"], dir.path()).unwrap(), 1);
        assert_eq!(fs::read(new_path(&dir.path().join("deno.exe"))).unwrap(), b"first");
    }
}
