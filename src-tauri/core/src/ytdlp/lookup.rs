use std::ffi::OsString;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::{base_args, Input};
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
}

impl LookupError {
    /// Coarse mapping until W05's full yt-dlp error classifier lands.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Spawn(SpawnError::Blocked(_)) => "tool_blocked",
            Self::Spawn(SpawnError::Missing(_)) => "tools_missing",
            Self::Spawn(SpawnError::TimedOut(_)) => "network",
            Self::Failed { stderr }
                if ["getaddrinfo", "Unable to download", "timed out", "Connection"]
                    .iter()
                    .any(|needle| stderr.contains(needle)) =>
            {
                "network"
            }
            _ => "lookup_failed",
        }
    }
}

pub fn lookup_args(paths: &ToolPaths, input: &Input, whole_playlist: bool) -> Vec<OsString> {
    let mut args = base_args(paths);
    args.extend(["-J".into(), "--flat-playlist".into()]);
    match input {
        Input::Search(query) => {
            // The query is part of the `ytsearchN:` operand, so it can never be read as a flag.
            args.push(format!("ytsearch{SEARCH_RESULTS}:{query}").into());
        }
        Input::Url { url, has_video, .. } => {
            if *has_video && !whole_playlist {
                args.push("--no-playlist".into());
            }
            args.extend(["-I".into(), format!("1:{PLAYLIST_LIMIT}").into(), "--".into(), url.into()]);
        }
    }
    args
}

fn card(entry: &Value) -> Option<VideoCard> {
    let live = entry.get("live_status").and_then(Value::as_str);
    if matches!(live, Some("is_live" | "is_upcoming")) {
        return None; // nothing to download yet
    }
    let id = entry.get("id")?.as_str()?.to_owned();
    let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(VideoCard {
        url: text("webpage_url")
            .or_else(|| text("url"))
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}")),
        title: text("title").unwrap_or_else(|| id.clone()),
        channel: text("channel").or_else(|| text("uploader")),
        duration_s: entry.get("duration").and_then(Value::as_f64).map(|d| d.round() as u64),
        thumbnail: format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"),
        id,
    })
}

pub fn parse_lookup(json: &Value, input: &Input, whole_playlist: bool) -> Result<Lookup, LookupError> {
    let is_playlist = json.get("_type").and_then(Value::as_str) == Some("playlist");
    let has_playlist = matches!(input, Input::Url { has_video: true, has_list: true, .. }) && !whole_playlist;
    if !is_playlist {
        let item = card(json).ok_or_else(|| LookupError::Output("video without id".into()))?;
        return Ok(Lookup {
            kind: LookupKind::Video,
            title: None,
            items: vec![item],
            truncated: false,
            has_playlist,
        });
    }
    let entries = json
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| LookupError::Output("playlist without entries".into()))?;
    let items: Vec<VideoCard> = entries.iter().filter_map(card).collect();
    let kind = if matches!(input, Input::Search(_)) { LookupKind::Search } else { LookupKind::Playlist };
    let total = json.get("playlist_count").and_then(Value::as_u64);
    Ok(Lookup {
        kind,
        title: (kind == LookupKind::Playlist)
            .then(|| json.get("title").and_then(Value::as_str).map(str::to_owned))
            .flatten(),
        truncated: kind == LookupKind::Playlist && total.is_some_and(|t| t as usize > entries.len()),
        items,
        has_playlist: false,
    })
}

pub async fn lookup(paths: &ToolPaths, input: &Input, whole_playlist: bool) -> Result<Lookup, LookupError> {
    let output = process::run(
        process::command(paths.ytdlp()).args(lookup_args(paths, input, whole_playlist)),
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

    #[test]
    fn every_call_points_yt_dlp_at_deno_and_ffmpeg() {
        let args = strings(lookup_args(&paths(), &Input::Search("x".into()), false));
        assert!(args.windows(2).any(|w| w == ["--js-runtimes", r"deno:C:\bin\deno.exe"]));
        assert!(args.windows(2).any(|w| w == ["--ffmpeg-location", r"C:\bin"]));
        assert!(args.windows(2).any(|w| w == ["--encoding", "utf-8"]));
    }

    #[test]
    fn search_uses_ytsearch_operand() {
        let args = strings(lookup_args(&paths(), &Input::Search("-rm 鄧麗君".into()), false));
        assert_eq!(args.last().unwrap(), "ytsearch10:-rm 鄧麗君");
    }

    #[test]
    fn mix_link_downloads_one_song_unless_whole_playlist() {
        let input = url_input("https://www.youtube.com/watch?v=abc&list=RDabc");
        let one = strings(lookup_args(&paths(), &input, false));
        assert!(one.contains(&"--no-playlist".to_owned()));
        let all = strings(lookup_args(&paths(), &input, true));
        assert!(!all.contains(&"--no-playlist".to_owned()));
        assert!(all.windows(2).any(|w| w == ["-I", "1:200"]));
        assert_eq!(&all[all.len() - 2..], ["--", "https://www.youtube.com/watch?v=abc&list=RDabc"]);
    }

    #[test]
    fn parses_search_results_and_skips_live() {
        let json: Value = serde_json::from_str(
            r#"{"_type":"playlist","title":"q","entries":[
              {"id":"a1","url":"https://www.youtube.com/watch?v=a1","title":"月亮代表我的心","channel":"鄧麗君","duration":205.0},
              {"id":"live","title":"直播","live_status":"is_live"},
              {"id":"b2","title":"No channel","uploader":"someone"}]}"#,
        )
        .unwrap();
        let result = parse_lookup(&json, &Input::Search("q".into()), false).unwrap();
        assert_eq!(result.kind, LookupKind::Search);
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].duration_s, Some(205));
        assert_eq!(result.items[0].thumbnail, "https://i.ytimg.com/vi/a1/mqdefault.jpg");
        assert_eq!(result.items[1].channel.as_deref(), Some("someone"));
        assert_eq!(result.items[1].url, "https://www.youtube.com/watch?v=b2");
        assert!(!result.truncated && result.title.is_none());
    }

    #[test]
    fn long_playlist_is_marked_truncated() {
        let json: Value = serde_json::from_str(
            r#"{"_type":"playlist","title":"老歌","playlist_count":350,"entries":[{"id":"a"},{"id":"b"}]}"#,
        )
        .unwrap();
        let result =
            parse_lookup(&json, &url_input("https://www.youtube.com/playlist?list=PL1"), false).unwrap();
        assert_eq!(result.kind, LookupKind::Playlist);
        assert_eq!(result.title.as_deref(), Some("老歌"));
        assert!(result.truncated);
    }

    #[test]
    fn single_video_from_mix_link_offers_whole_playlist() {
        let json: Value = serde_json::from_str(
            r#"{"_type":"video","id":"abc","title":"t","webpage_url":"https://www.youtube.com/watch?v=abc"}"#,
        )
        .unwrap();
        let input = url_input("https://www.youtube.com/watch?v=abc&list=RDabc");
        let result = parse_lookup(&json, &input, false).unwrap();
        assert_eq!(result.kind, LookupKind::Video);
        assert!(result.has_playlist);
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
}
