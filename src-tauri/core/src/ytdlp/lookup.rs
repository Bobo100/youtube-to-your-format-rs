use std::ffi::OsString;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::errors::{classify, YtError};
use super::{base_args, Input};
use crate::tools::update::NeedsNewYtdlp;
use crate::process::{self, SpawnError};
use crate::tools::ToolPaths;

pub const SEARCH_RESULTS: usize = 10;
pub const PLAYLIST_LIMIT: usize = 200;
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCard {
    pub id: String,
    pub url: String,
    pub title: String,
    pub channel: Option<String>,
    pub duration_s: Option<u64>,
    pub thumbnail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LookupKind {
    Video,
    Playlist,
    Search,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lookup {
    pub kind: LookupKind,
    pub title: Option<String>,
    pub items: Vec<VideoCard>,
    /// The playlist has more entries than were listed.
    pub truncated: bool,
    /// Private, members-only, deleted or live entries left out of `items`.
    pub skipped: usize,
    /// A single video opened from a link that also names a playlist (e.g. a Mix):
    /// the UI offers "整個清單".
    pub has_playlist: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error("tool: {0}")]
    Spawn(#[from] SpawnError),
    #[error("yt-dlp failed: {stderr}")]
    Failed { stderr: String },
    #[error("unexpected output: {0}")]
    Output(String),
    #[error("video cannot be downloaded (live, private or members-only)")]
    Unavailable,
    #[error("not a YouTube link")]
    NotYoutube,
}

impl LookupError {
    pub fn code(&self, searching: bool) -> &'static str {
        let broken = if searching { "search_failed" } else { "lookup_failed" };
        match self {
            Self::Spawn(SpawnError::Blocked(_)) => "tool_blocked",
            Self::Spawn(SpawnError::Missing(_)) => "tools_missing",
            Self::Spawn(SpawnError::TimedOut(_)) => "network",
            Self::Failed { stderr } => match classify(stderr) {
                YtError::Extractor | YtError::NotFound => broken,
                other => other.code(),
            },
            Self::Unavailable => "unavailable",
            Self::NotYoutube => "not_youtube",
            _ => broken,
        }
    }
}

impl NeedsNewYtdlp for LookupError {
    fn needs_new_ytdlp(&self) -> bool {
        matches!(self, Self::Failed { stderr } if classify(stderr).may_be_fixed_by_update())
    }
}

pub fn lookup_args(
    paths: &ToolPaths,
    input: &Input,
    whole_playlist: bool,
    cookies: Option<&std::path::Path>,
) -> Vec<OsString> {
    let mut args = base_args(paths);
    args.extend(["-J".into(), "--flat-playlist".into()]);
    // Age-restricted videos fail at lookup already, before any download could use them.
    if let Some(cookies) = cookies {
        args.extend([OsString::from("--cookies"), cookies.as_os_str().to_owned()]);
    }
    match input {
        Input::Search(query) => {
            // The query is part of the `ytsearchN:` operand, so it can never be read as a flag.
            args.push(format!("ytsearch{SEARCH_RESULTS}:{query}").into());
        }
        Input::Url { url, has_video, .. } => {
            if *has_video && !whole_playlist {
                args.push("--no-playlist".into());
            }
            // One past the limit: channel tabs and Mixes report no playlist_count,
            // so an extra entry is the only way to know the list was cut.
            let range = format!("1:{}", PLAYLIST_LIMIT + 1);
            args.extend(["-I".into(), range.into(), "--".into(), url.into()]);
        }
        Input::NotYoutube => {}
    }
    args
}

enum Entry {
    Card(VideoCard),
    /// A real video that cannot be downloaded right now.
    Skipped,
    /// Not a video at all (e.g. a channel tab).
    Ignored,
}

fn is_video_id(id: &str) -> bool {
    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn entry(value: &Value, require_duration: bool) -> Entry {
    let text = |key: &str| value.get(key).and_then(Value::as_str);
    if !matches!(text("_type"), None | Some("url" | "video")) {
        return Entry::Ignored;
    }
    let Some(id) = text("id").filter(|id| is_video_id(id)) else {
        return Entry::Ignored;
    };
    let live = matches!(text("live_status"), Some("is_live" | "is_upcoming" | "post_live"));
    let restricted = matches!(
        text("availability"),
        Some("private" | "subscriber_only" | "premium_only" | "needs_auth")
    );
    let title = text("title").filter(|t| !matches!(*t, "[Private video]" | "[Deleted video]"));
    let duration = value.get("duration").and_then(Value::as_f64).map(|d| d.round() as u64);
    // Flat search results never mark ongoing streams as live, but they have no duration.
    let missing_duration = require_duration && duration.is_none();
    let Some(title) = title.filter(|_| !live && !restricted && !missing_duration) else {
        return Entry::Skipped;
    };
    Entry::Card(VideoCard {
        url: text("webpage_url")
            .or_else(|| text("url"))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}")),
        title: title.to_owned(),
        channel: text("channel").or_else(|| text("uploader")).map(str::to_owned),
        duration_s: duration,
        thumbnail: format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"),
        id: id.to_owned(),
    })
}

pub fn parse_lookup(json: &Value, input: &Input, whole_playlist: bool) -> Result<Lookup, LookupError> {
    let is_playlist = json.get("_type").and_then(Value::as_str) == Some("playlist");
    if !is_playlist {
        let Entry::Card(item) = entry(json, false) else {
            return Err(LookupError::Unavailable);
        };
        return Ok(Lookup {
            kind: LookupKind::Video,
            title: None,
            items: vec![item],
            truncated: false,
            skipped: 0,
            has_playlist: matches!(input, Input::Url { has_video: true, has_list: true, .. }) && !whole_playlist,
        });
    }
    let entries = json
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| LookupError::Output("playlist without entries".into()))?;
    let searching = matches!(input, Input::Search(_));
    let kind = if searching { LookupKind::Search } else { LookupKind::Playlist };
    let over_limit = entries.len() > PLAYLIST_LIMIT;
    let mut items = Vec::new();
    let mut skipped = 0;
    for value in entries.iter().take(PLAYLIST_LIMIT) {
        match entry(value, searching) {
            Entry::Card(card) => items.push(card),
            Entry::Skipped => skipped += 1,
            Entry::Ignored => {}
        }
    }
    let total = json.get("playlist_count").and_then(Value::as_u64);
    Ok(Lookup {
        kind,
        title: (kind == LookupKind::Playlist)
            .then(|| json.get("title").and_then(Value::as_str).map(str::to_owned))
            .flatten(),
        truncated: kind == LookupKind::Playlist
            && (over_limit || total.is_some_and(|t| t as usize > PLAYLIST_LIMIT)),
        items,
        skipped,
        has_playlist: false,
    })
}

pub async fn lookup(paths: &ToolPaths, input: &Input, whole_playlist: bool) -> Result<Lookup, LookupError> {
    if *input == Input::NotYoutube {
        return Err(LookupError::NotYoutube);
    }
    let cookies = super::download::prepared_cookies();
    let output = process::run(
        process::command(paths.ytdlp()).args(lookup_args(paths, input, whole_playlist, cookies.as_deref())),
        LOOKUP_TIMEOUT,
    )
    .await?;
    if !output.status.success() {
        return Err(LookupError::Failed {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let json: Value =
        serde_json::from_slice(&output.stdout).map_err(|e| LookupError::Output(e.to_string()))?;
    parse_lookup(&json, input, whole_playlist)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn paths() -> ToolPaths {
        ToolPaths { bin: PathBuf::from(r"C:\bin") }
    }

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    fn url_input(url: &str) -> Input {
        Input::parse(url).unwrap()
    }

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn every_call_points_yt_dlp_at_deno_and_ffmpeg() {
        let args = strings(lookup_args(&paths(), &Input::Search("x".into()), false, None));
        assert!(args.windows(2).any(|w| w == ["--js-runtimes", r"deno:C:\bin\deno.exe"]));
        assert!(args.windows(2).any(|w| w == ["--ffmpeg-location", r"C:\bin"]));
        assert!(args.windows(2).any(|w| w == ["--encoding", "utf-8"]));
    }

    #[test]
    fn search_uses_ytsearch_operand() {
        let args = strings(lookup_args(&paths(), &Input::Search("-rm 鄧麗君".into()), false, None));
        assert_eq!(args.last().unwrap(), "ytsearch10:-rm 鄧麗君");
    }

    #[test]
    fn mix_link_downloads_one_song_unless_whole_playlist() {
        let input = url_input("https://www.youtube.com/watch?v=abc&list=RDabc");
        let one = strings(lookup_args(&paths(), &input, false, None));
        assert!(one.contains(&"--no-playlist".to_owned()));
        let all = strings(lookup_args(&paths(), &input, true, None));
        assert!(!all.contains(&"--no-playlist".to_owned()));
        assert!(all.windows(2).any(|w| w == ["-I", "1:201"]));
        assert_eq!(&all[all.len() - 2..], ["--", "https://www.youtube.com/watch?v=abc&list=RDabc"]);
    }

    #[test]
    fn search_skips_live_and_durationless_streams() {
        // Shapes observed from yt-dlp 2026.08.19 flat search output.
        let result = parse_lookup(
            &json(
                r#"{"_type":"playlist","title":"q","entries":[
                  {"_type":"url","ie_key":"Youtube","id":"IiFm7AWP9n4","url":"https://www.youtube.com/watch?v=IiFm7AWP9n4","title":"月亮代表我的心","channel":"鄧麗君","duration":205.0,"live_status":null},
                  {"_type":"url","id":"4xDzrJKXOOY","title":"lofi radio","duration":null,"live_status":null},
                  {"_type":"url","id":"blAFxjhg62k","title":"直播","live_status":"is_live","duration":null},
                  {"_type":"url","id":"znX5F09ysr0","title":"No channel","uploader":"someone","duration":206}]}"#,
            ),
            &Input::Search("q".into()),
            false,
        )
        .unwrap();
        assert_eq!(result.kind, LookupKind::Search);
        let ids: Vec<&str> = result.items.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["IiFm7AWP9n4", "znX5F09ysr0"]);
        assert_eq!(result.skipped, 2);
        assert_eq!(result.items[0].thumbnail, "https://i.ytimg.com/vi/IiFm7AWP9n4/mqdefault.jpg");
        assert_eq!(result.items[1].channel.as_deref(), Some("someone"));
        assert_eq!(result.items[1].url, "https://www.youtube.com/watch?v=znX5F09ysr0");
    }

    #[test]
    fn playlist_skips_private_members_only_and_deleted() {
        let result = parse_lookup(
            &json(
                r#"{"_type":"playlist","title":"老歌","playlist_count":4,"entries":[
                  {"_type":"url","id":"Brpwrk8kHH4","title":null,"duration":null},
                  {"_type":"url","id":"aaaaaaaaaaa","title":"會員","availability":"subscriber_only","duration":100},
                  {"_type":"url","id":"bbbbbbbbbbb","title":"[Deleted video]","duration":null},
                  {"_type":"url","id":"ccccccccccc","title":"好歌","duration":180}]}"#,
            ),
            &url_input("https://www.youtube.com/playlist?list=PL1"),
            false,
        )
        .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].title, "好歌");
        assert_eq!(result.skipped, 3);
        assert!(!result.truncated);
    }

    #[test]
    fn channel_tabs_are_not_videos() {
        let result = parse_lookup(
            &json(
                r#"{"_type":"playlist","title":"Channel","entries":[
                  {"_type":"playlist","id":"UCXuqSBlHAE6Xw-yeJA0Tunw","title":"Channel - Videos","url":null}]}"#,
            ),
            &url_input("https://www.youtube.com/@x/videos"),
            false,
        )
        .unwrap();
        assert!(result.items.is_empty());
        assert_eq!(result.skipped, 0);
    }

    #[test]
    fn over_limit_without_count_is_truncated() {
        let entries: Vec<String> = (0..=PLAYLIST_LIMIT)
            .map(|i| format!(r#"{{"_type":"url","id":"{i:0>11}","title":"t","duration":1}}"#))
            .collect();
        let result = parse_lookup(
            &json(&format!(r#"{{"_type":"playlist","title":"Mix","entries":[{}]}}"#, entries.join(","))),
            &url_input("https://www.youtube.com/@x/videos"),
            false,
        )
        .unwrap();
        assert!(result.truncated);
        assert_eq!(result.items.len(), PLAYLIST_LIMIT);
    }

    #[test]
    fn known_long_playlist_is_truncated() {
        let result = parse_lookup(
            &json(r#"{"_type":"playlist","title":"老歌","playlist_count":350,"entries":[{"id":"ccccccccccc","title":"t"}]}"#),
            &url_input("https://www.youtube.com/playlist?list=PL1"),
            false,
        )
        .unwrap();
        assert_eq!(result.title.as_deref(), Some("老歌"));
        assert!(result.truncated);
    }

    #[test]
    fn single_video_from_mix_link_offers_whole_playlist() {
        let input = url_input("https://www.youtube.com/watch?v=IiFm7AWP9n4&list=RDIiFm7AWP9n4");
        let video = json(
            r#"{"_type":"video","id":"IiFm7AWP9n4","title":"t","webpage_url":"https://www.youtube.com/watch?v=IiFm7AWP9n4","live_status":"not_live"}"#,
        );
        let result = parse_lookup(&video, &input, false).unwrap();
        assert_eq!(result.kind, LookupKind::Video);
        assert!(result.has_playlist);
    }

    #[test]
    fn live_single_video_is_unavailable() {
        let video = json(r#"{"_type":"video","id":"4xDzrJKXOOY","title":"lofi","live_status":"is_live"}"#);
        let err = parse_lookup(&video, &url_input("https://youtu.be/4xDzrJKXOOY"), false).unwrap_err();
        assert_eq!(err.code(false), "unavailable");
    }

    #[test]
    fn http_4xx_is_a_wrong_link_not_a_network_problem() {
        let wrong = LookupError::Failed {
            stderr: "ERROR: [youtube:tab] @x: Unable to download API page: HTTP Error 404: Not Found".into(),
        };
        assert_eq!(wrong.code(false), "lookup_failed");
        assert_eq!(wrong.code(true), "search_failed");
        let offline = LookupError::Failed {
            stderr: "ERROR: Unable to download webpage: <urlopen error [Errno 11001] getaddrinfo failed>".into(),
        };
        assert_eq!(offline.code(false), "network");
    }

    /// Real network + yt-dlp. Needs tools installed by the app (`prepare_tools`).
    #[tokio::test]
    #[ignore]
    async fn real_search_returns_cards() {
        let paths = ToolPaths::from_env().unwrap();
        let result = lookup(&paths, &Input::Search("鄧麗君 月亮代表我的心".into()), false).await.unwrap();
        assert_eq!(result.kind, LookupKind::Search);
        assert!(!result.items.is_empty());
        assert!(result.items.iter().all(|c| !c.title.is_empty()));
    }

    #[tokio::test]
    #[ignore]
    async fn real_channel_home_lists_videos() {
        let paths = ToolPaths::from_env().unwrap();
        let result = lookup(&paths, &url_input("https://www.youtube.com/@YouTube"), false).await.unwrap();
        assert_eq!(result.kind, LookupKind::Playlist);
        assert!(!result.items.is_empty());
        assert!(result.items.iter().all(|c| is_video_id(&c.id)));
    }
}

