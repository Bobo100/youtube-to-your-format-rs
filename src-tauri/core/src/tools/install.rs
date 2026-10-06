use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

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

/// Puts `target.new` in place of `target`, keeping the previous file as
/// `target.old` for rollback. Windows cannot overwrite a running exe, so the
/// caller must make sure nothing is executing `target`.
pub fn replace_with_new(target: &Path) -> io::Result<()> {
    let new = new_path(target);
    OpenOptions::new().write(true).open(&new)?.sync_all()?;
    if target.exists() {
        let old = old_path(target);
        if old.exists() {
            fs::remove_file(&old)?;
        }
        fs::rename(target, &old)?;
    }
    fs::rename(&new, target)
}

/// Copies the first archive entry whose name ends with one of `wanted` suffixes
/// into `<dir>/<file name>.new`. Returns how many of `wanted` were found.
pub fn extract_zip(archive: &Path, wanted: &[&str], dir: &Path) -> io::Result<usize> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(io::Error::other)?;
    let mut found = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(io::Error::other)?;
        let Some(name) = matching(entry.name(), wanted) else {
            continue;
        };
        write_new(&mut entry, &dir.join(name))?;
        found += 1;
    }
    Ok(found)
}

pub fn extract_7z(archive: &Path, wanted: &[&str], dir: &Path) -> io::Result<usize> {
    let mut reader =
        sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty())
            .map_err(io::Error::other)?;
    let mut found = 0;
    reader
        .for_each_entries(|entry, data| {
            // Solid archive: every entry must be read through, even unwanted ones.
            match matching(entry.name(), wanted) {
                Some(name) if !entry.is_directory() => {
                    write_new(data, &dir.join(name))?;
                    found += 1;
                }
                _ => {
                    io::copy(data, &mut io::sink())?;
                }
            }
            Ok(true)
        })
        .map_err(io::Error::other)?;
    Ok(found)
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
        {
            let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("deno.exe", opts).unwrap();
            zip.write_all(b"deno").unwrap();
            zip.start_file("README.md", opts).unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        assert_eq!(extract_zip(&archive, &["deno.exe"], dir.path()).unwrap(), 1);
        assert_eq!(fs::read(new_path(&dir.path().join("deno.exe"))).unwrap(), b"deno");
    }
}
