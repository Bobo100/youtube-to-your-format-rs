/// yt-dlp versions are dotted numbers: stable `2026.08.19`, nightly `2026.08.19.231807`.
fn parse(version: &str) -> Option<Vec<u64>> {
    version.trim().split('.').map(|part| part.parse().ok()).collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numerically_not_lexically() {
        assert!(is_newer("2026.10.01", "2026.09.30"));
        assert!(is_newer("2026.08.19.231807", "2026.08.19"));
        assert!(!is_newer("2026.08.19", "2026.08.19"));
        assert!(!is_newer("2026.08.09", "2026.08.19"));
    }

    #[test]
    fn unknown_current_version_is_replaced() {
        assert!(is_newer("2026.08.19", "garbage"));
        assert!(!is_newer("garbage", "2026.08.19"));
    }
}
