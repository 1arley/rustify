use std::process::Command;

use serde::Deserialize;
use url::Url;

use crate::catalog::{Track, TrackId, youtube_id};
use crate::library::SavedPlaylist;

const MAX_PLAYLIST_TRACKS: usize = 500;

#[derive(Deserialize)]
struct PlaylistInfo {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    entries: Option<Vec<Option<Video>>>,
}

#[derive(Deserialize)]
struct Video {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    thumbnail: Option<String>,
}

pub(crate) fn import_playlist(raw_url: &str) -> Result<SavedPlaylist, String> {
    let (playlist_id, url) = playlist_url(raw_url)?;
    let limit = MAX_PLAYLIST_TRACKS.to_string();
    let output = Command::new("yt-dlp")
        .args([
            "--ignore-config",
            "--no-warnings",
            "--no-call-home",
            "--skip-download",
            "--flat-playlist",
            "--dump-single-json",
            "--playlist-end",
            &limit,
            "--socket-timeout",
            "20",
            "--retries",
            "1",
            "--",
            &url,
        ])
        .output()
        .map_err(|error| format!("Importar playlist do YouTube requer yt-dlp: {error}"))?;
    if !output.status.success() {
        return Err("Não foi possível importar essa playlist do YouTube.".to_owned());
    }
    let info: PlaylistInfo = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Resposta inválida ao importar playlist do YouTube: {error}"))?;
    parse_playlist(playlist_id, url, info)
}

fn playlist_url(raw_url: &str) -> Result<(String, String), String> {
    let parsed = Url::parse(raw_url.trim())
        .map_err(|_| "Cole um link válido de playlist do YouTube.".to_owned())?;
    let valid_host = matches!(
        parsed.host_str(),
        Some(
            "youtube.com" | "www.youtube.com" | "music.youtube.com" | "m.youtube.com" | "youtu.be"
        )
    );
    if !valid_host
        || !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
    {
        return Err("Cole um link válido de playlist do YouTube.".to_owned());
    }
    let playlist_id = parsed
        .query_pairs()
        .find_map(|(key, value)| (key == "list").then(|| value.into_owned()))
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or_else(|| "O link precisa conter o identificador de uma playlist.".to_owned())?;
    let url = format!("https://www.youtube.com/playlist?list={playlist_id}");
    Ok((playlist_id, url))
}

fn parse_playlist(
    playlist_id: String,
    source_url: String,
    info: PlaylistInfo,
) -> Result<SavedPlaylist, String> {
    let mut tracks = Vec::new();
    for video in info.entries.unwrap_or_default().into_iter().flatten() {
        let (Some(id), Some(title)) = (video.id, video.title) else {
            continue;
        };
        if !youtube_id(&id) || title.trim().is_empty() {
            continue;
        }
        if tracks
            .iter()
            .any(|track: &Track| track.id == TrackId::Youtube(id.clone()))
        {
            continue;
        }
        tracks.push(Track::from_youtube(
            id,
            title,
            video.channel.or(video.uploader).unwrap_or_default(),
            video.duration.unwrap_or_default(),
            video.thumbnail,
        ));
    }
    if tracks.is_empty() {
        return Err("A playlist não contém faixas disponíveis.".to_owned());
    }
    let name = info
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| "Playlist do YouTube".to_owned());
    let artwork_url = info
        .thumbnail
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .or_else(|| tracks.first().and_then(Track::artwork_url));
    Ok(SavedPlaylist {
        id: format!("youtube:playlist:{playlist_id}"),
        name,
        description: String::new(),
        artwork_url,
        source_url: Some(source_url),
        tracks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_youtube_playlist_links_and_rejects_other_hosts() {
        assert_eq!(
            playlist_url("https://music.youtube.com/playlist?list=PLabc_123&si=unused").unwrap(),
            (
                "PLabc_123".to_owned(),
                "https://www.youtube.com/playlist?list=PLabc_123".to_owned()
            )
        );
        assert!(playlist_url("https://example.com/playlist?list=PLabc_123").is_err());
        assert!(playlist_url("https://youtube.com/watch?v=abc").is_err());
    }

    #[test]
    fn parses_flat_playlist_metadata_without_audio_files() {
        let info: PlaylistInfo = serde_json::from_str(
            r#"{
                "title":"Mix",
                "thumbnail":"https://img.youtube.test/playlist.jpg",
                "entries":[
                    {"id":"abcdefghijk","title":"Song A","channel":"Artist","duration":120},
                    {"id":"abcdefghijk","title":"Song A","channel":"Artist","duration":120},
                    null,
                    {"id":"invalid","title":"Unavailable"}
                ]
            }"#,
        )
        .unwrap();
        let playlist = parse_playlist(
            "PLabc_123".to_owned(),
            "https://www.youtube.com/playlist?list=PLabc_123".to_owned(),
            info,
        )
        .unwrap();

        assert_eq!(playlist.name, "Mix");
        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].artist_names(), "Artist");
        assert!(matches!(&playlist.tracks[0].id, TrackId::Youtube(_)));
    }
}
