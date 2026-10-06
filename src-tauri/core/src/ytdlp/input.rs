use url::Url;

/// What the user typed (or pasted) into the one big box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Url { url: String, has_video: bool, has_list: bool },
    Search(String),
    /// A link to another site: thumbnails, CSP and the download flow assume YouTube.
    NotYoutube,
}

const YOUTUBE_HOSTS: [&str; 5] = [
    "youtube.com",
    "youtu.be",
    "youtube-nocookie.com",
    "m.youtube.com",
    "music.youtube.com",
];

impl Input {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        // Shared messages ("歌名 https://youtu.be/xxx") carry the link inside text.
        let Some(link) = find_link(text) else {
            return Some(Self::Search(text.to_owned()));
        };
        let with_scheme = if link.starts_with("http://") || link.starts_with("https://") {
            link.to_owned()
        } else {
            format!("https://{link}")
        };
        let Ok(mut url) = Url::parse(&with_scheme) else {
            return Some(Self::Search(text.to_owned()));
        };
        let host = url.host_str().unwrap_or_default().trim_start_matches("www.").to_owned();
        if !YOUTUBE_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
            return Some(Self::NotYoutube);
        }
        let path = url.path().to_owned();
        let has_video = url.query_pairs().any(|(k, _)| k == "v")
            || host == "youtu.be"
            || ["/shorts/", "/live/", "/embed/"].iter().any(|p| path.starts_with(p));
        let has_list = url.query_pairs().any(|(k, _)| k == "list");
        if is_channel_home(&path) {
            // A channel's home lists its tabs (Videos / Live / Shorts), not videos.
            url.set_path(&format!("{}/videos", path.trim_end_matches('/')));
        }
        Some(Self::Url { url: url.into(), has_video, has_list })
    }
}

fn find_link(text: &str) -> Option<&str> {
    text.split_whitespace().find(|word| {
        word.starts_with("http://")
            || word.starts_with("https://")
            || YOUTUBE_HOSTS
                .iter()
                .any(|h| word.starts_with(&format!("{h}/")) || word.starts_with(&format!("www.{h}/")))
    })
}

fn is_channel_home(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [handle] => handle.starts_with('@'),
        [kind, _] => ["channel", "c", "user"].contains(kind),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> (String, bool, bool) {
        match Input::parse(text) {
            Some(Input::Url { url, has_video, has_list }) => (url, has_video, has_list),
            other => panic!("{text} parsed as {other:?}"),
        }
    }

    #[test]
    fn words_are_a_search() {
        assert_eq!(Input::parse("  鄧麗君 月亮代表我的心 "), Some(Input::Search("鄧麗君 月亮代表我的心".into())));
        assert_eq!(Input::parse("   "), None);
        assert!(matches!(Input::parse("youtube 教學"), Some(Input::Search(_))));
    }

    #[test]
    fn recognises_video_and_list_parts() {
        assert!(url("https://www.youtube.com/watch?v=abc").1);
        let (_, v, l) = url("https://www.youtube.com/watch?v=abc&list=RDabc");
        assert!(v && l);
        let (_, v, l) = url("https://www.youtube.com/playlist?list=PL1");
        assert!(!v && l);
        assert!(url("https://youtu.be/abc?si=x").1);
        assert!(url("https://www.youtube.com/shorts/abc").1);
        assert!(url("https://www.youtube-nocookie.com/embed/abc?list=PL1").1);
        assert!(url("https://music.youtube.com/watch?v=abc").1);
    }

    #[test]
    fn link_inside_shared_text_is_used() {
        let (u, v, _) = url("鄧麗君 月亮代表我的心 https://youtu.be/abc");
        assert_eq!(u, "https://youtu.be/abc");
        assert!(v);
        assert_eq!(url("https://youtu.be/abc 好聽").0, "https://youtu.be/abc");
        assert_eq!(url("youtu.be/abc").0, "https://youtu.be/abc");
    }

    #[test]
    fn channel_home_opens_the_videos_tab() {
        assert_eq!(url("https://www.youtube.com/@TeresaTeng").0, "https://www.youtube.com/@TeresaTeng/videos");
        assert_eq!(
            url("https://music.youtube.com/channel/UC123").0,
            "https://music.youtube.com/channel/UC123/videos"
        );
        assert_eq!(url("https://www.youtube.com/@x/shorts").0, "https://www.youtube.com/@x/shorts");
    }

    #[test]
    fn other_sites_are_rejected() {
        assert_eq!(Input::parse("https://vimeo.com/123"), Some(Input::NotYoutube));
        assert_eq!(Input::parse("https://notyoutube.com/watch?v=1"), Some(Input::NotYoutube));
    }
}
