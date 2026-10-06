use std::path::PathBuf;

/// `<Downloads>\YouTube`. Uses the real Downloads known folder, which people
/// (and OneDrive) often move off `%USERPROFILE%\Downloads`.
pub fn default_output_dir() -> PathBuf {
    downloads_dir()
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("YouTube")
}

#[cfg(windows)]
fn downloads_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    unsafe {
        let mut raw: windows_sys::core::PWSTR = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT as _, std::ptr::null_mut(), &mut raw);
        if hr != 0 || raw.is_null() {
            if !raw.is_null() {
                CoTaskMemFree(raw as _);
            }
            return None;
        }
        let len = (0..).take_while(|&i| *raw.add(i) != 0).count();
        let path = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw as _);
        Some(PathBuf::from(path))
    }
}

#[cfg(not(windows))]
fn downloads_dir() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dir_is_a_youtube_folder_under_downloads() {
        let dir = default_output_dir();
        assert!(dir.ends_with("YouTube"));
        assert!(dir.is_absolute(), "{dir:?}");
    }
}
