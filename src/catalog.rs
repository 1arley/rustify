use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

const API_BASE: &str = "https://api.audius.co/v1";
const APP_NAME: &str = "rea_music";

fn search_tracks(query: &str) -> Result<Vec<Track>, String> {
    let mut url =
        Url::parse(&format!("{API_BASE}/tracks/search")).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("query", query)
        .append_pair("limit", "20")
        .append_pair("app_name", APP_NAME);

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build();
    let mut response = ureq::Agent::new_with_config(config)
        .get(url.as_str())
        .call()
        .map_err(|error| format!("Não foi possível pesquisar no Audius: {error}"))?;

    let status = response.status().as_u16();
    if status >= 400 {
        return Err(format!("Audius respondeu com HTTP {status}."));
    }

    let response = response
        .body_mut()
        .read_json::<SearchResponse>()
        .map_err(|error| format!("Resposta de busca do Audius inválida: {error}"))?;
    Ok(response
        .data
        .into_iter()
        .filter(|track| {
            track.streamable && !track.stream_gated && matches!(track.id, TrackId::Audius(_))
        })
        .collect())
}

fn youtube_tracks(query: &str) -> Result<Vec<Track>, String> {
    #[derive(Deserialize)]
    struct Video {
        id: String,
        title: String,
        channel: Option<String>,
        duration: Option<f64>,
    }
    let output = std::process::Command::new("yt-dlp")
        .args([
            "--ignore-config",
            "--socket-timeout",
            "10",
            "--retries",
            "1",
            "--flat-playlist",
            "--dump-json",
            "--",
            &format!("ytsearch12:{query}"),
        ])
        .output()
        .map_err(|e| format!("Busca do YouTube requer yt-dlp: {e}"))?;
    if !output.status.success() {
        return Err("O YouTube não respondeu à busca.".into());
    }
    let mut tracks = Vec::new();
    for row in output
        .stdout
        .split(|b| *b == b'\n')
        .filter(|row| !row.is_empty())
    {
        let video: Video = serde_json::from_slice(row)
            .map_err(|e| format!("Resposta do YouTube inválida: {e}"))?;
        if youtube_id(&video.id) {
            tracks.push(Track {
                id: TrackId::Youtube(video.id),
                name: video.title,
                user: User {
                    name: video.channel.unwrap_or_default(),
                },
                genre: "YouTube".into(),
                duration: video.duration.unwrap_or_default(),
                permalink: String::new(),
                streamable: true,
                stream_gated: false,
            });
        }
    }
    Ok(tracks)
}

pub(crate) fn youtube_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Track {
    pub id: TrackId,
    #[serde(rename = "title")]
    pub name: String,
    #[serde(default)]
    user: User,
    #[serde(default)]
    genre: String,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    permalink: String,
    #[serde(default, rename = "is_streamable")]
    streamable: bool,
    #[serde(default, rename = "is_stream_gated")]
    stream_gated: bool,
}

impl Track {
    pub(crate) fn artist_names(&self) -> &str {
        &self.user.name
    }

    pub(crate) fn album_name(&self) -> &str {
        &self.genre
    }

    pub(crate) fn duration_micros(&self) -> i64 {
        (self.duration.max(0.0) * 1_000_000.0) as i64
    }

    pub(crate) fn stream_url(&self) -> String {
        self.id.stream_url()
    }

    pub(crate) fn permalink(&self) -> String {
        let TrackId::Audius(id) = &self.id else {
            return self.stream_url();
        };
        if self.permalink.starts_with('/')
            && !self.permalink.starts_with("//")
            && !self.permalink.contains("..")
        {
            format!("https://audius.co{}", self.permalink)
        } else {
            format!("https://audius.co/tracks/{id}")
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct User {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<Track>,
}

#[derive(Clone, Copy)]
pub(crate) enum Source {
    Youtube,
    Audius,
}
impl Source {
    pub fn search(self, query: &str) -> Result<Vec<Track>, String> {
        match self {
            Self::Youtube => youtube_tracks(query),
            Self::Audius => search_tracks(query),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) enum TrackId {
    Audius(String),
    Youtube(String),
}
impl TryFrom<String> for TrackId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_valid_id(&value) {
            Ok(Self::Audius(value))
        } else if let Some(id) = value.strip_prefix("yt:").filter(|id| youtube_id(id)) {
            Ok(Self::Youtube(id.into()))
        } else {
            Err("Identificador de faixa inválido")
        }
    }
}
impl From<TrackId> for String {
    fn from(id: TrackId) -> String {
        match id {
            TrackId::Youtube(id) => format!("yt:{id}"),
            TrackId::Audius(id) => id,
        }
    }
}
impl TrackId {
    pub fn stream_url(&self) -> String {
        match self {
            Self::Youtube(id) => format!("https://www.youtube.com/watch?v={id}"),
            Self::Audius(id) => format!("{API_BASE}/tracks/{id}/stream?app_name={APP_NAME}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_likes_keep_their_source_after_reopening() {
        let raw = r#"[{"id":"abc","title":"Mesmo título"},{"id":"yt:abcdefghijk","title":"Mesmo título"}]"#;
        let tracks: Vec<Track> = serde_json::from_str(raw).unwrap();
        let reopened: Vec<Track> =
            serde_json::from_slice(&serde_json::to_vec(&tracks).unwrap()).unwrap();
        assert_eq!(
            reopened[0].stream_url(),
            "https://api.audius.co/v1/tracks/abc/stream?app_name=rea_music"
        );
        assert_eq!(
            reopened[1].stream_url(),
            "https://www.youtube.com/watch?v=abcdefghijk"
        );
        assert!(serde_json::from_str::<TrackId>(r#""yt:../bad""#).is_err());
    }
}
