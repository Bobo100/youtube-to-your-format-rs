use reqwest::Client;
use serde_json::Value;

use super::checksum::parse_sums;
use super::ToolError;

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub exe_url: String,
    pub sums_url: String,
}

pub fn from_json(json: &Value, exe: &str, sums: &str) -> Option<Release> {
    let version = json.get("tag_name")?.as_str()?.to_owned();
    let asset_url = |name: &str| {
        json.get("assets")?
            .as_array()?
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(name))?
            .get("browser_download_url")?
            .as_str()
            .map(str::to_owned)
    };
    Some(Release {
        version,
        exe_url: asset_url(exe)?,
        sums_url: asset_url(sums)?,
    })
}

pub async fn latest(client: &Client, repo: &str, exe: &str, sums: &str) -> Result<Release, ToolError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(ToolError::Http(response.status().as_u16()));
    }
    let json: Value = response.json().await?;
    from_json(&json, exe, sums).ok_or_else(|| ToolError::Release(format!("{repo}: missing {exe}")))
}

pub async fn expected_sha256(client: &Client, release: &Release, exe: &str) -> Result<String, ToolError> {
    let response = client.get(&release.sums_url).send().await?;
    if !response.status().is_success() {
        return Err(ToolError::Http(response.status().as_u16()));
    }
    let text = response.text().await?;
    parse_sums(&text)
        .remove(exe)
        .ok_or_else(|| ToolError::Release(format!("{} has no hash for {exe}", release.version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_named_assets_from_release_json() {
        let json: Value = serde_json::from_str(
            r#"{"tag_name":"2026.08.19","assets":[
                {"name":"yt-dlp","browser_download_url":"https://x/yt-dlp"},
                {"name":"yt-dlp.exe","browser_download_url":"https://x/yt-dlp.exe"},
                {"name":"SHA2-256SUMS","browser_download_url":"https://x/SHA2-256SUMS"}]}"#,
        )
        .unwrap();
        assert_eq!(
            from_json(&json, "yt-dlp.exe", "SHA2-256SUMS"),
            Some(Release {
                version: "2026.08.19".into(),
                exe_url: "https://x/yt-dlp.exe".into(),
                sums_url: "https://x/SHA2-256SUMS".into(),
            })
        );
    }

    #[test]
    fn missing_asset_yields_none() {
        let json: Value = serde_json::from_str(r#"{"tag_name":"v1","assets":[]}"#).unwrap();
        assert_eq!(from_json(&json, "yt-dlp.exe", "SHA2-256SUMS"), None);
    }
}
