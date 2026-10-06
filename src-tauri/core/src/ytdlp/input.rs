use url::Url;

/// What the user typed into the one big box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Url { url: String, has_video: bool, has_list: bool },
    Search(String),
}

impl Input {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let candidate = if text.starts_with("http://") || text.starts_with("https://") {
            text.to_owned()
        } else if looks_like_bare_youtube_link(text) {
            format!("https://{text}")
        } else {
            return Some(Self::Search(text.to_owned()));
        };
        let Ok(url) = Url::parse(&candidate) else {
            return Some(Self::Search(text.to_owned()));
        };
        let host = url.host_str().unwrap_or_default();
        let has_video = url.query_pairs().any(|(k, _)| k == "v")
            || host.ends_with("youtu.be")
            || url.path().starts_with("/shorts/")
            || url.path().starts_with("/live/");
        let has_list = url.query_pairs().any(|(k, _)| k == "list");
        Some(Self::Url { url: url.into(), has_video, has_list })
    }
}

/// People paste links without the scheme (`youtu.be/xxx`, `www.youtube.com/...`).
fn looks_like_bare_youtube_link(text: &str) -> bool {
    !text.contains(char::is_whitespace)
        && ["youtu.be/", "youtube.com/", "www.youtube.com/", "m.youtube.com/", "music.youtube.com/"]
            .iter()
            .any(|prefix| text.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> (bool, bool) {
        match Input::parse(text) {
            Some(Input::Url { has_video, has_list, .. }) => (has_video, has_list),
            other => panic!("{text} parsed as {other:?}"),
        }
    }

    #[test]
    fn words_are_a_search() {
        assert_eq!(Input::parse("  鄧麗君 月亮代表我的心 "), Some(Input::Search("鄧麗君 月亮代表我的心".into())));
        assert_eq!(Input::parse("   "), None);
    }

    #[test]
    fn recognises_video_and_list_parts() {
        assert_eq!(url("https://www.youtube.com/watch?v=abc"), (true, false));
        assert_eq!(url("https://www.youtube.com/watch?v=abc&list=RDabc"), (true, true));
        assert_eq!(url("https://www.youtube.com/playlist?list=PL1"), (false, true));
        assert_eq!(url("https://youtu.be/abc?si=x"), (true, false));
        assert_eq!(url("https://www.youtube.com/shorts/abc"), (true, false));
        assert_eq!(url("https://www.youtube.com/@channel"), (false, false));
    }

    #[test]
    fn bare_links_get_a_scheme() {
        match Input::parse("youtu.be/abc") {
            Some(Input::Url { url, has_video, .. }) => {
                assert_eq!(url, "https://youtu.be/abc");
                assert!(has_video);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(Input::parse("youtube 教學"), Some(Input::Search(_))));
    }
}
