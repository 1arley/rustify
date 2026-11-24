use std::collections::{HashMap, HashSet};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

const API_BASE: &str = "https://api.audius.co/v1";
const APP_NAME: &str = "rea_music";
const MIN_SUGGESTION_SECS: f64 = 40.0;
const MAX_SUGGESTION_SECS: f64 = 900.0;
const MAX_SEED_ARTIST_SUGGESTIONS: usize = 3;
const MAX_OTHER_ARTIST_SUGGESTIONS: usize = 2;
const DEFAULT_RECOMMENDATION_LIMIT: usize = 15;

const ARTIST_SUFFIXES: &[&str] = &[
    " - topic",
    " official",
    " oficial",
    " offiziell",
    " vevo",
    "vevo",
    " music",
    " records",
    " tv",
    "tv",
];

const BRACKET_META_WORDS: &[&str] = &[
    "official",
    "oficial",
    "video",
    "audio",
    "lyrics",
    "lyric",
    "remaster",
    "remastered",
    "4k",
    "hd",
    "live",
    "visualizer",
    "clipe",
    "legendado",
    "upgrade",
    "music",
    "version",
    "single",
    "edit",
    "explicit",
    "hq",
    "mv",
    "feat",
    "ft",
    "featuring",
];

const NON_MUSIC_TITLE_TERMS: &[&str] = &[
    "full album",
    "álbum completo",
    "album completo",
    "1 hour",
    "2 hours",
    "10 hours",
    "podcast",
    "reaction",
];

const TAG_STOPWORDS: &[&str] = &[
    "free", "playlist", "music", "track", "song", "audio", "mp3", "new",
];

fn audius_agent(timeout_secs: u64) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(config)
}

fn fetch_audius_list(url: Url, timeout_secs: u64) -> Result<Vec<Track>, String> {
    let mut response = audius_agent(timeout_secs)
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

fn search_tracks(query: &str) -> Result<Vec<Track>, String> {
    let mut url =
        Url::parse(&format!("{API_BASE}/tracks/search")).map_err(|error| error.to_string())?;
    url.query_pairs_mut()
        .append_pair("query", query)
        .append_pair("limit", "20")
        .append_pair("app_name", APP_NAME);
    fetch_audius_list(url, 20)
}

fn fetch_audius_track(track_id: &str) -> Result<Track, String> {
    if !is_valid_id(track_id) {
        return Err("Identificador Audius inválido.".into());
    }
    let mut url =
        Url::parse(&format!("{API_BASE}/tracks/{track_id}")).map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("app_name", APP_NAME);

    let mut response = audius_agent(10)
        .get(url.as_str())
        .call()
        .map_err(|error| format!("Não foi possível consultar faixa no Audius: {error}"))?;
    let status = response.status().as_u16();
    if status >= 400 {
        return Err(format!("Audius respondeu com HTTP {status}."));
    }
    let response = response
        .body_mut()
        .read_json::<SingleTrackResponse>()
        .map_err(|error| format!("Resposta do Audius inválida: {error}"))?;
    Ok(response.data)
}

fn trending_tracks(genre: Option<&str>) -> Result<Vec<Track>, String> {
    let mut url =
        Url::parse(&format!("{API_BASE}/tracks/trending")).map_err(|error| error.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(genre) = genre.map(str::trim).filter(|g| !g.is_empty()) {
            pairs.append_pair("genre", genre);
        }
        pairs
            .append_pair("limit", "20")
            .append_pair("app_name", APP_NAME);
    }
    fetch_audius_list(url, 10)
}

fn resolve_video_artist(channel: Option<&str>, uploader: Option<&str>, title: &str) -> String {
    if let Some(ch) = channel.map(str::trim).filter(|s| !s.is_empty()) {
        return ch.to_owned();
    }
    if let Some(up) = uploader.map(str::trim).filter(|s| !s.is_empty()) {
        return up.to_owned();
    }
    extract_title_artist(title).unwrap_or_default().to_owned()
}

fn extract_title_artist(title: &str) -> Option<&str> {
    for sep in [" - ", " – ", " — "] {
        if let Some((left, right)) = title.split_once(sep) {
            let left = left.trim();
            let right = right.trim();
            if !left.is_empty() && !right.is_empty() {
                return Some(left);
            }
        }
    }
    None
}

fn yt_dlp_name() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

pub(crate) fn bundled_yt_dlp() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(yt_dlp_name());
    candidate.is_file().then_some(candidate)
}

pub(crate) fn yt_dlp_command() -> std::process::Command {
    let program = bundled_yt_dlp().unwrap_or_else(|| std::path::PathBuf::from(yt_dlp_name()));
    std::process::Command::new(program)
}

#[derive(Deserialize)]
struct YoutubeResult {
    id: String,
    title: String,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
}

fn track_from_youtube_result(video: YoutubeResult) -> Option<Track> {
    if !youtube_id(&video.id) {
        return None;
    }
    let artist = video.artist.unwrap_or_else(|| {
        resolve_video_artist(
            video.channel.as_deref(),
            video.uploader.as_deref(),
            &video.title,
        )
    });
    let mut track = Track::from_youtube(
        video.id,
        video.title,
        artist,
        video.duration.unwrap_or_default(),
        None,
    );
    if let Some(album) = video.album.filter(|album| !album.trim().is_empty()) {
        track.genre = album;
    }
    Some(track)
}

fn run_yt_dlp(target: &str, limit: usize) -> Result<Vec<Track>, String> {
    run_yt_dlp_with_flat_playlist(target, limit, true)
}

fn run_yt_dlp_with_flat_playlist(
    target: &str,
    limit: usize,
    flat_playlist: bool,
) -> Result<Vec<Track>, String> {
    run_yt_dlp_with_flat_playlist_cancellable(target, limit, flat_playlist, None)
}

fn run_yt_dlp_with_flat_playlist_cancellable(
    target: &str,
    limit: usize,
    flat_playlist: bool,
    cancelled: Option<&AtomicBool>,
) -> Result<Vec<Track>, String> {
    let limit_arg = limit.to_string();
    let mut command = yt_dlp_command();
    command.args([
        "--ignore-config",
        "--socket-timeout",
        "10",
        "--retries",
        "1",
        "--skip-download",
        "--playlist-end",
        &limit_arg,
        "--dump-json",
    ]);
    if flat_playlist {
        command.arg("--flat-playlist");
    }
    command.args(["--", target]);
    let output = run_command_cancellable(&mut command, cancelled)
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
        let Ok(video) = serde_json::from_slice::<YoutubeResult>(row) else {
            continue;
        };
        if let Some(track) = track_from_youtube_result(video) {
            tracks.push(track);
        }
    }
    Ok(tracks)
}

fn run_command_cancellable(
    command: &mut Command,
    cancelled: Option<&AtomicBool>,
) -> Result<Output, String> {
    let Some(cancelled) = cancelled else {
        return command.output().map_err(|error| error.to_string());
    };
    if cancelled.load(Ordering::Acquire) {
        return Err("Preparação cancelada.".to_owned());
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait_with_output();
            return Err("Preparação cancelada.".to_owned());
        }
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().map_err(|error| error.to_string()),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}

fn youtube_tracks(query: &str) -> Result<Vec<Track>, String> {
    run_yt_dlp(&format!("ytsearch12:{query}"), 12)
}

fn youtube_music_search_url(query: &str, section: &str) -> Result<String, String> {
    let mut url =
        Url::parse("https://music.youtube.com/search").map_err(|error| error.to_string())?;
    url.query_pairs_mut().append_pair("q", query);
    url.set_fragment(Some(section));
    Ok(url.into())
}

fn youtube_music_tracks(query: &str, section: &str) -> Result<Vec<Track>, String> {
    let url = youtube_music_search_url(query, section)?;
    run_yt_dlp_with_flat_playlist(&url, 12, false)
}

fn prioritized_search_results(
    songs: Vec<Track>,
    audius: Vec<Track>,
    videos: Vec<Track>,
    conventional: Vec<Track>,
) -> Vec<Track> {
    let music = merge_search_results(songs, audius);
    if !music.is_empty() {
        return music;
    }
    if videos.is_empty() {
        conventional
    } else {
        merge_search_results(videos, conventional)
    }
}

pub(crate) fn search_all(query: &str) -> Result<Vec<Track>, String> {
    let (youtube, audius) = std::thread::scope(|scope| {
        let youtube = scope.spawn(|| youtube_music_tracks(query, "songs"));
        let audius = scope.spawn(|| search_tracks(query));
        (youtube.join(), audius.join())
    });
    let youtube = youtube.unwrap_or_else(|_| Err("A busca do YouTube falhou.".to_owned()));
    let audius = audius.unwrap_or_else(|_| Err("A busca do Audius falhou.".to_owned()));
    let (songs, songs_error) =
        youtube.map_or_else(|error| (Vec::new(), Some(error)), |tracks| (tracks, None));
    let (audius, audius_error) =
        audius.map_or_else(|error| (Vec::new(), Some(error)), |tracks| (tracks, None));
    let music = merge_search_results(songs.clone(), audius.clone());
    if !music.is_empty() {
        return Ok(music);
    }
    let (videos, videos_error) = youtube_music_tracks(query, "videos")
        .map_or_else(|error| (Vec::new(), Some(error)), |tracks| (tracks, None));
    if !videos.is_empty() {
        return Ok(prioritized_search_results(
            songs,
            audius,
            videos,
            Vec::new(),
        ));
    }
    let (conventional, conventional_error) = youtube_tracks(query)
        .map_or_else(|error| (Vec::new(), Some(error)), |tracks| (tracks, None));
    if conventional.is_empty() && songs_error.is_some() && audius_error.is_some() {
        let mut errors = [songs_error, audius_error, videos_error, conventional_error]
            .into_iter()
            .flatten();
        if let Some(error) = errors.next() {
            return Err(errors.fold(error, |all, error| format!("{all} {error}")));
        }
    }
    Ok(prioritized_search_results(
        songs,
        audius,
        videos,
        conventional,
    ))
}

fn merge_search_results(mut tracks: Vec<Track>, incoming: Vec<Track>) -> Vec<Track> {
    for track in incoming {
        let title = canonical_song_title(&track.name, track.artist_names());
        let artist = normalize_artist(track.artist_names(), &track.name);
        let duplicate = tracks.iter().any(|current| {
            current.same_identity(&track)
                || (!title.is_empty()
                    && !artist.is_empty()
                    && canonical_song_title(&current.name, current.artist_names()) == title
                    && normalize_artist(current.artist_names(), &current.name) == artist)
        });
        if !duplicate {
            tracks.push(track);
        }
    }
    tracks
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

pub(crate) fn is_spotify_base62_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn spotify_track_key(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn extract_spotify_track_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(id) = trimmed.strip_prefix("spotify:track:") {
        return is_spotify_base62_id(id).then(|| id.to_owned());
    }
    if is_spotify_base62_id(trimmed) {
        return Some(trimmed.to_owned());
    }
    let url = Url::parse(trimmed).ok()?;
    let mut segments = url.path_segments()?;
    while let Some(seg) = segments.next() {
        if seg == "track"
            && let Some(id) = segments.next()
            && is_spotify_base62_id(id)
        {
            return Some(id.to_owned());
        }
    }
    None
}

fn local_spotify_key(title: &str, artist: &str) -> String {
    let mut norm_artist = normalize_artist(artist, title);
    let mut norm_title = canonical_song_title(title, artist);
    if norm_artist.is_empty() {
        norm_artist = collapse_spaces(&artist.to_lowercase());
    }
    if norm_title.is_empty() {
        norm_title = collapse_spaces(&title.to_lowercase());
    }
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in norm_artist.bytes().chain([0_u8]).chain(norm_title.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("local_{hash:016x}")
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mood: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tags: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bpm: Option<f64>,
    #[serde(default)]
    duration: f64,
    #[serde(default)]
    permalink: String,
    #[serde(default)]
    artwork: Option<Artwork>,
    #[serde(default, rename = "is_streamable")]
    streamable: bool,
    #[serde(default, rename = "is_stream_gated")]
    stream_gated: bool,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct Artwork {
    #[serde(rename = "150x150", default)]
    small: Option<String>,
    #[serde(rename = "480x480", default)]
    medium: Option<String>,
    #[serde(rename = "1000x1000", default)]
    large: Option<String>,
}

impl Track {
    pub(crate) fn from_youtube(
        id: String,
        title: String,
        artist: String,
        duration_secs: f64,
        artwork_url: Option<String>,
    ) -> Self {
        debug_assert!(youtube_id(&id));
        let permalink = format!("https://www.youtube.com/watch?v={id}");
        let artwork = artwork_url
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
            .map(|url| Artwork {
                small: None,
                medium: Some(url),
                large: None,
            });
        Self {
            id: TrackId::Youtube(id),
            name: title,
            user: User { name: artist },
            genre: "YouTube".into(),
            mood: None,
            tags: None,
            bpm: None,
            duration: duration_secs.max(0.0),
            permalink,
            artwork,
            streamable: true,
            stream_gated: false,
        }
    }

    /// Constrói uma faixa preservando a identidade original do Spotify.
    pub(crate) fn from_spotify(
        spotify_id: Option<&str>,
        title: String,
        artist: String,
        album: String,
        duration_ms: u64,
        artwork_url: Option<String>,
    ) -> Self {
        let title = title.trim().to_owned();
        let artist = artist.trim().to_owned();
        let extracted_id = spotify_id.and_then(extract_spotify_track_id);
        let sp_key = extracted_id
            .clone()
            .unwrap_or_else(|| local_spotify_key(&title, &artist));

        let id = TrackId::Spotify(sp_key);
        let permalink = extracted_id
            .map(|id| format!("https://open.spotify.com/track/{id}"))
            .unwrap_or_default();
        let artwork = artwork_url
            .map(|url| url.trim().to_owned())
            .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
            .map(|url| Artwork {
                small: None,
                medium: Some(url),
                large: None,
            });
        let genre = if album.trim().is_empty() {
            "Spotify".to_owned()
        } else {
            album.trim().to_owned()
        };

        Self {
            id,
            name: title,
            user: User { name: artist },
            genre,
            mood: None,
            tags: None,
            bpm: None,
            duration: (duration_ms as f64) / 1000.0,
            permalink,
            artwork,
            streamable: true,
            stream_gated: false,
        }
    }

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

    /// Retorna `true` se duas faixas representam a mesma música por ID, permalink ou vínculo resolvido.
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        (!self.permalink.is_empty() && self.permalink == other.permalink)
            || self.id.same_identity(&other.id)
    }

    /// Retorna a URL da capa da faixa (Audius, Spotify ou miniatura padrão do YouTube).
    pub(crate) fn artwork_url(&self) -> Option<String> {
        if let Some(artwork) = &self.artwork {
            for candidate in [&artwork.medium, &artwork.small, &artwork.large] {
                if let Some(url) = candidate
                    .as_deref()
                    .map(str::trim)
                    .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
                {
                    return Some(url.to_owned());
                }
            }
        }
        match &self.id {
            TrackId::Youtube(id) => Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg")),
            TrackId::Audius(_) | TrackId::Spotify(_) => None,
        }
    }

    pub(crate) fn permalink(&self) -> String {
        if self.permalink.starts_with("https://") || self.permalink.starts_with("http://") {
            return self.permalink.clone();
        }
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

#[derive(Deserialize)]
struct SingleTrackResponse {
    data: Track,
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn alnum_compact(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn clean_artist_name(raw: &str) -> String {
    let mut s = collapse_spaces(&raw.to_lowercase());
    loop {
        let mut changed = false;
        for suffix in ARTIST_SUFFIXES {
            if let Some(stripped) = s.strip_suffix(suffix) {
                let trimmed = stripped
                    .trim_end_matches(|c: char| !c.is_alphanumeric())
                    .trim();
                if !trimmed.is_empty() {
                    s = trimmed.to_owned();
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            break;
        }
    }
    collapse_spaces(&s)
}

pub(crate) fn normalize_artist(artist: &str, title: &str) -> String {
    let cleaned = clean_artist_name(artist);
    if cleaned.is_empty() || cleaned == "youtube" {
        if let Some(from_title) = extract_title_artist(title) {
            return clean_artist_name(from_title);
        }
    }
    cleaned
}

fn is_meta_bracket(inner: &str) -> bool {
    let lower = inner.to_lowercase();
    if lower.contains("ao vivo")
        || lower.contains("official video")
        || lower.contains("music video")
        || lower.contains("lyric video")
        || lower.contains("audio oficial")
        || lower.contains("clipe oficial")
    {
        return true;
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|w| BRACKET_META_WORDS.contains(&w))
}

fn strip_metadata_brackets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch == '(' || ch == '[' {
            let close = if ch == '(' { ')' } else { ']' };
            let inner_start = idx + ch.len_utf8();
            let mut depth = 1usize;
            let mut inner_end = None;
            for (j, c) in chars.by_ref() {
                if c == ch {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                    if depth == 0 {
                        inner_end = Some(j);
                        break;
                    }
                }
            }
            if let Some(end) = inner_end {
                let inner = &input[inner_start..end];
                if !is_meta_bracket(inner) {
                    out.push(ch);
                    out.push_str(inner);
                    out.push(close);
                } else {
                    out.push(' ');
                }
            } else {
                out.push_str(&input[idx..]);
                break;
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn strip_artist_from_title(title: &str, artist: &str) -> String {
    let artist_clean = clean_artist_name(artist);
    let artist_key = alnum_compact(&artist_clean);
    if artist_key.is_empty() {
        return title.to_owned();
    }
    for sep in [" - ", " – ", " — ", ": "] {
        if let Some((left, right)) = title.split_once(sep) {
            let left_t = left.trim();
            let right_t = right.trim();
            if !left_t.is_empty() && !right_t.is_empty() && alnum_compact(left_t) == artist_key {
                return right_t.to_owned();
            }
        }
        if let Some((left, right)) = title.rsplit_once(sep) {
            let left_t = left.trim();
            let right_t = right.trim();
            if !left_t.is_empty() && !right_t.is_empty() && alnum_compact(right_t) == artist_key {
                return left_t.to_owned();
            }
        }
    }
    if let Some((left, right)) = title.split_once(':') {
        let left_t = left.trim();
        let right_t = right.trim();
        if !left_t.is_empty() && !right_t.is_empty() && alnum_compact(left_t) == artist_key {
            return right_t.to_owned();
        }
    }
    title.to_owned()
}

fn strip_feat_suffix(s: &str) -> &str {
    let mut end = s.len();
    for marker in [" ft. ", " feat. ", " featuring ", " ft ", " feat "] {
        if let Some(pos) = s.find(marker) {
            end = end.min(pos);
        }
    }
    &s[..end]
}

pub(crate) fn canonical_song_title(title: &str, artist: &str) -> String {
    let no_brackets = strip_metadata_brackets(title);
    let collapsed = collapse_spaces(&no_brackets.to_lowercase());
    let no_artist = strip_artist_from_title(&collapsed, artist);
    let no_feat = strip_feat_suffix(&no_artist);
    no_feat
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_valid_music_candidate(track: &Track, seed_secs: f64, seed_title_lower: &str) -> bool {
    if track.duration > 0.0 {
        let allow_short = seed_secs > 0.0 && seed_secs < 45.0;
        let allow_long = seed_secs > 600.0;
        if (!allow_short && track.duration < MIN_SUGGESTION_SECS)
            || (!allow_long && track.duration > MAX_SUGGESTION_SECS)
        {
            return false;
        }
    }
    let title_lower = track.name.to_lowercase();
    for term in NON_MUSIC_TITLE_TERMS {
        if title_lower.contains(term) && !seed_title_lower.contains(term) {
            return false;
        }
    }
    true
}

fn diversify_by_artist(candidates: Vec<Track>, seed_artist_norm: &str, limit: usize) -> Vec<Track> {
    let mut pool: Vec<(Track, String)> = candidates
        .into_iter()
        .map(|track| {
            let artist = normalize_artist(track.artist_names(), &track.name);
            (track, artist)
        })
        .collect();

    let mut result = Vec::with_capacity(limit.min(pool.len()));
    let mut artist_counts: HashMap<String, usize> = HashMap::new();
    let mut last_artist = seed_artist_norm.to_owned();

    while result.len() < limit {
        let next_idx = pool.iter().position(|(_, artist)| {
            if artist.is_empty() {
                return true;
            }
            if !last_artist.is_empty() && artist == &last_artist {
                return false;
            }
            let cap = if !seed_artist_norm.is_empty() && artist == seed_artist_norm {
                MAX_SEED_ARTIST_SUGGESTIONS
            } else {
                MAX_OTHER_ARTIST_SUGGESTIONS
            };
            artist_counts.get(artist).copied().unwrap_or(0) < cap
        });

        let Some(idx) = next_idx else {
            break;
        };
        let (track, artist) = pool.remove(idx);
        if !artist.is_empty() {
            *artist_counts.entry(artist.clone()).or_insert(0) += 1;
            last_artist = artist;
        }
        result.push(track);
    }

    while result.len() < limit && !pool.is_empty() {
        let idx = pool
            .iter()
            .position(|(_, artist)| {
                artist.is_empty() || last_artist.is_empty() || artist != &last_artist
            })
            .unwrap_or(0);
        let (track, artist) = pool.remove(idx);
        if !artist.is_empty() {
            last_artist = artist;
        }
        result.push(track);
    }

    result
}

pub(crate) fn curate_recommendations(
    seed_id: &TrackId,
    seed_title: &str,
    seed_artist: &str,
    seed_duration_micros: i64,
    candidates: Vec<Track>,
    exclude: &[TrackId],
    limit: usize,
) -> Vec<Track> {
    if limit == 0 {
        return Vec::new();
    }
    let seed_canonical = canonical_song_title(seed_title, seed_artist);
    let seed_artist_norm = normalize_artist(seed_artist, seed_title);
    let seed_title_lower = seed_title.to_lowercase();
    let seed_secs = (seed_duration_micros.max(0) as f64) / 1_000_000.0;

    let mut seen_ids = HashSet::new();
    let mut seen_canonicals = HashSet::new();
    let mut filtered = Vec::new();

    for track in candidates {
        if &track.id == seed_id || exclude.contains(&track.id) || !seen_ids.insert(track.id.clone())
        {
            continue;
        }
        let cand_canonical = canonical_song_title(&track.name, track.artist_names());
        if cand_canonical.is_empty() {
            continue;
        }
        if !seed_canonical.is_empty() {
            let cand_with_seed_artist = canonical_song_title(&track.name, seed_artist);
            if cand_canonical == seed_canonical || cand_with_seed_artist == seed_canonical {
                continue;
            }
        }
        if !is_valid_music_candidate(&track, seed_secs, &seed_title_lower) {
            continue;
        }
        if !seen_canonicals.insert(cand_canonical) {
            continue;
        }
        filtered.push(track);
    }

    diversify_by_artist(filtered, &seed_artist_norm, limit)
}

fn parse_tags(raw: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(raw) = raw else { return out };
    for part in raw.split(',') {
        let tag = part.trim().to_lowercase();
        if tag.len() >= 2 && !TAG_STOPWORDS.contains(&tag.as_str()) && !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

fn score_audius_candidate(
    candidate: &Track,
    seed_tags: &HashSet<String>,
    seed_genre: &str,
    seed_mood: &str,
    seed_artist_norm: &str,
    seed_bpm: Option<f64>,
    seed_secs: f64,
) -> i32 {
    let mut score = 0i32;

    let cand_tags: HashSet<String> = parse_tags(candidate.tags.as_deref()).into_iter().collect();
    let overlap = cand_tags.intersection(seed_tags).count() as i32;
    score += overlap * 30;

    if !seed_genre.is_empty() && candidate.genre.trim().eq_ignore_ascii_case(seed_genre) {
        score += 25;
    }
    if !seed_mood.is_empty()
        && candidate
            .mood
            .as_deref()
            .is_some_and(|m| m.trim().eq_ignore_ascii_case(seed_mood))
    {
        score += 20;
    }
    let cand_artist = normalize_artist(candidate.artist_names(), &candidate.name);
    if !seed_artist_norm.is_empty() && cand_artist == seed_artist_norm {
        score += 15;
    }
    if let (Some(sb), Some(cb)) = (seed_bpm, candidate.bpm)
        && sb > 0.0
        && cb > 0.0
    {
        let diff = (sb - cb).abs();
        let half_diff = (sb - cb * 2.0).abs().min((sb * 2.0 - cb).abs());
        let best_diff = diff.min(half_diff);
        if best_diff <= 10.0 {
            score += 12;
        } else if best_diff <= 20.0 {
            score += 6;
        }
    }
    if seed_secs > 0.0 && candidate.duration > 0.0 && (seed_secs - candidate.duration).abs() <= 45.0
    {
        score += 8;
    }
    score
}

fn recommend_youtube_tracks(
    video_id: &str,
    seed_id: &TrackId,
    seed_title: &str,
    seed_artist: &str,
    seed_duration_micros: i64,
    exclude: &[TrackId],
) -> Result<Vec<Track>, String> {
    if !youtube_id(video_id) {
        return Err("Identificador do YouTube inválido.".into());
    }
    let mut candidates = Vec::new();
    let mut any_ok = false;
    let mut last_err = None;

    let sources = [
        format!("https://music.youtube.com/watch?v={video_id}&list=RDAMVM{video_id}"),
        format!("https://www.youtube.com/watch?v={video_id}&list=RD{video_id}"),
    ];

    for url in &sources {
        match run_yt_dlp(url, 25) {
            Ok(tracks) => {
                any_ok = true;
                candidates.extend(tracks);
            }
            Err(err) => last_err = Some(err),
        }
        if curate_recommendations(
            seed_id,
            seed_title,
            seed_artist,
            seed_duration_micros,
            candidates.clone(),
            exclude,
            DEFAULT_RECOMMENDATION_LIMIT,
        )
        .len()
            >= 6
        {
            break;
        }
    }

    if curate_recommendations(
        seed_id,
        seed_title,
        seed_artist,
        seed_duration_micros,
        candidates.clone(),
        exclude,
        DEFAULT_RECOMMENDATION_LIMIT,
    )
    .len()
        < 6
    {
        let fallback_query = if seed_artist.is_empty()
            || seed_title
                .to_lowercase()
                .contains(&seed_artist.to_lowercase())
        {
            format!("{seed_title} mix")
        } else {
            format!("{seed_artist} {seed_title} mix")
        };
        let fallback_query = fallback_query.trim();
        if !fallback_query.is_empty() {
            match youtube_tracks(fallback_query) {
                Ok(tracks) => {
                    any_ok = true;
                    candidates.extend(tracks);
                }
                Err(err) => last_err = Some(err),
            }
        }
    }

    if !any_ok {
        return Err(last_err.unwrap_or_else(|| "Falha ao buscar sugestões.".into()));
    }

    Ok(curate_recommendations(
        seed_id,
        seed_title,
        seed_artist,
        seed_duration_micros,
        candidates,
        exclude,
        DEFAULT_RECOMMENDATION_LIMIT,
    ))
}

fn recommend_audius_tracks(
    track_id: &str,
    seed_id: &TrackId,
    seed_title: &str,
    seed_artist: &str,
    seed_duration_micros: i64,
    exclude: &[TrackId],
) -> Result<Vec<Track>, String> {
    let details = fetch_audius_track(track_id).ok();

    let effective_title = details
        .as_ref()
        .map(|t| t.name.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(seed_title);
    let effective_artist = details
        .as_ref()
        .map(|t| t.artist_names())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(seed_artist);
    let genre = details
        .as_ref()
        .map(|t| t.genre.trim().to_owned())
        .unwrap_or_default();
    let mood = details
        .as_ref()
        .and_then(|t| t.mood.as_deref())
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let seed_tag_list = parse_tags(details.as_ref().and_then(|t| t.tags.as_deref()));
    let seed_bpm = details.as_ref().and_then(|t| t.bpm);
    let seed_secs = details
        .as_ref()
        .map(|t| t.duration)
        .filter(|d| *d > 0.0)
        .unwrap_or_else(|| (seed_duration_micros.max(0) as f64) / 1_000_000.0);

    let mut queries = Vec::new();
    if !seed_tag_list.is_empty() {
        let mut parts: Vec<String> = seed_tag_list.iter().take(3).cloned().collect();
        if !mood.is_empty() && !parts.iter().any(|p| p.eq_ignore_ascii_case(&mood)) {
            parts.push(mood.clone());
        } else if !genre.is_empty() && !parts.iter().any(|p| p.eq_ignore_ascii_case(&genre)) {
            parts.push(genre.clone());
        }
        queries.push(parts.join(" "));
    }
    if !effective_artist.trim().is_empty() {
        let q = if !genre.is_empty() {
            format!("{} {}", effective_artist.trim(), genre)
        } else {
            effective_artist.trim().to_owned()
        };
        if !queries.contains(&q) {
            queries.push(q);
        }
    }
    if queries.is_empty() && !effective_title.trim().is_empty() {
        queries.push(effective_title.trim().to_owned());
    }

    let mut candidates = Vec::new();
    let mut any_ok = false;
    let mut last_err = None;

    for query in &queries {
        match search_tracks(query) {
            Ok(tracks) => {
                any_ok = true;
                candidates.extend(tracks);
            }
            Err(err) => last_err = Some(err),
        }
    }

    match trending_tracks(if genre.is_empty() {
        None
    } else {
        Some(genre.as_str())
    }) {
        Ok(tracks) => {
            any_ok = true;
            candidates.extend(tracks);
        }
        Err(err) => last_err = Some(err),
    }

    if !any_ok {
        return Err(last_err.unwrap_or_else(|| "Falha ao buscar sugestões no Audius.".into()));
    }

    let seed_tags_set: HashSet<String> = seed_tag_list.into_iter().collect();
    let seed_artist_norm = normalize_artist(effective_artist, effective_title);
    let mut scored: Vec<(i32, Track)> = candidates
        .into_iter()
        .map(|track| {
            let score = score_audius_candidate(
                &track,
                &seed_tags_set,
                &genre,
                &mood,
                &seed_artist_norm,
                seed_bpm,
                seed_secs,
            );
            (score, track)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let ranked: Vec<Track> = scored.into_iter().map(|(_, track)| track).collect();

    Ok(curate_recommendations(
        seed_id,
        effective_title,
        effective_artist,
        seed_duration_micros,
        ranked,
        exclude,
        DEFAULT_RECOMMENDATION_LIMIT,
    ))
}

fn select_best_youtube_match(
    candidates: Vec<Track>,
    title: &str,
    artist: &str,
    duration_micros: i64,
) -> Option<String> {
    let target_secs = (duration_micros.max(0) as f64) / 1_000_000.0;
    let seed_title_lower = title.to_lowercase();
    let seed_title_canon = canonical_song_title(title, artist);
    let seed_artist_norm = normalize_artist(artist, title);
    let primary_artist = seed_artist_norm
        .split(',')
        .next()
        .map(str::trim)
        .unwrap_or_default();

    let yt_candidates: Vec<(usize, Track)> = candidates
        .into_iter()
        .enumerate()
        .filter(|(_, track)| matches!(&track.id, TrackId::Youtube(id) if youtube_id(id)))
        .collect();
    if yt_candidates.is_empty() {
        return None;
    }

    let valid_music: Vec<(usize, Track)> = yt_candidates
        .iter()
        .filter(|(_, track)| is_valid_music_candidate(track, target_secs, &seed_title_lower))
        .cloned()
        .collect();
    let pool = if valid_music.is_empty() {
        yt_candidates
    } else {
        valid_music
    };

    let mut best: Option<(i32, usize, String)> = None;
    for (idx, cand) in pool {
        let TrackId::Youtube(video_id) = cand.id.clone() else {
            continue;
        };
        let mut score = 0i32;

        let cand_title_canon = canonical_song_title(&cand.name, cand.artist_names());
        let cand_title_with_seed = canonical_song_title(&cand.name, artist);
        if !seed_title_canon.is_empty() {
            if cand_title_canon == seed_title_canon || cand_title_with_seed == seed_title_canon {
                score += 50;
            } else if !cand_title_canon.is_empty()
                && (cand_title_canon.contains(&seed_title_canon)
                    || seed_title_canon.contains(&cand_title_canon))
            {
                score += 25;
            }
        }

        let cand_artist_norm = normalize_artist(cand.artist_names(), &cand.name);
        let cand_name_lower = cand.name.to_lowercase();
        if !seed_artist_norm.is_empty() {
            if cand_artist_norm == seed_artist_norm
                || (!cand_artist_norm.is_empty()
                    && (cand_artist_norm.contains(&seed_artist_norm)
                        || seed_artist_norm.contains(&cand_artist_norm)))
                || cand_name_lower.contains(&seed_artist_norm)
            {
                score += 30;
            } else if !primary_artist.is_empty()
                && (cand_artist_norm.contains(primary_artist)
                    || cand_name_lower.contains(primary_artist))
            {
                score += 25;
            }
        }

        if target_secs > 0.0 && cand.duration > 0.0 {
            let diff = (target_secs - cand.duration).abs();
            if diff <= 5.0 {
                score += 40;
            } else if diff <= 15.0 {
                score += 25;
            } else if diff <= 35.0 {
                score += 10;
            } else if diff > 120.0 {
                score -= 50;
            }
        }

        for term in [
            "live",
            "ao vivo",
            "cover",
            "karaoke",
            "instrumental",
            "remix",
            "sped up",
            "slowed",
        ] {
            if cand_name_lower.contains(term) && !seed_title_lower.contains(term) {
                score -= 25;
            }
        }

        let ch_lower = cand.artist_names().to_lowercase();
        if ch_lower.ends_with("- topic")
            || ch_lower.contains("official")
            || ch_lower.contains("vevo")
        {
            score += 10;
        }

        let replace = match &best {
            None => true,
            Some((best_score, best_idx, _)) => {
                score > *best_score || (score == *best_score && idx < *best_idx)
            }
        };
        if replace {
            best = Some((score, idx, video_id));
        }
    }

    best.map(|(_, _, id)| id)
}

/// Busca no YouTube o vídeo mais fiel aos metadados importados do Spotify.
pub(crate) fn resolve_spotify_to_youtube(
    title: &str,
    artist: &str,
    duration_micros: i64,
) -> Result<String, String> {
    resolve_spotify_to_youtube_inner(title, artist, duration_micros, None)
}

pub(crate) fn resolve_spotify_to_youtube_cancellable(
    title: &str,
    artist: &str,
    duration_micros: i64,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    resolve_spotify_to_youtube_inner(title, artist, duration_micros, Some(cancelled))
}

fn resolve_spotify_to_youtube_inner(
    title: &str,
    artist: &str,
    duration_micros: i64,
    cancelled: Option<&AtomicBool>,
) -> Result<String, String> {
    let started = std::time::Instant::now();
    let title_t = title.trim();
    let artist_t = artist.trim();
    let query = if artist_t.is_empty() {
        title_t.to_owned()
    } else if title_t.is_empty() {
        artist_t.to_owned()
    } else {
        format!("{artist_t} - {title_t}")
    };
    if query.is_empty() {
        return Err("Faixa sem título ou artista para buscar áudio.".to_owned());
    }
    let candidates = run_yt_dlp_with_flat_playlist_cancellable(
        &format!("ytsearch5:{query}"),
        5,
        true,
        cancelled,
    )?;
    let result = select_best_youtube_match(candidates, title_t, artist_t, duration_micros)
        .ok_or_else(|| "Não foi possível encontrar áudio correspondente no YouTube.".to_owned());
    eprintln!(
        "playback identity-search: {} ms",
        started.elapsed().as_millis()
    );
    result
}

/// Resolve a URL temporária de áudio para uma faixa do YouTube sem baixar o arquivo.
#[cfg(test)]
pub(crate) fn youtube_media_url(video_id: &str) -> Result<String, String> {
    youtube_media_url_inner(video_id, None)
}

pub(crate) fn youtube_media_url_cancellable(
    video_id: &str,
    cancelled: &AtomicBool,
) -> Result<String, String> {
    youtube_media_url_inner(video_id, Some(cancelled))
}

fn youtube_media_url_inner(
    video_id: &str,
    cancelled: Option<&AtomicBool>,
) -> Result<String, String> {
    let started = std::time::Instant::now();
    let mut command = yt_dlp_command();
    command
        .args([
            "--ignore-config",
            "--socket-timeout",
            "10",
            "--retries",
            "1",
            "--format",
            "ba[protocol=m3u8_native]/b[protocol=m3u8_native]/ba/b",
            "--get-url",
            "--",
        ])
        .arg(format!("https://www.youtube.com/watch?v={video_id}"));
    let output = run_command_cancellable(&mut command, cancelled)
        .map_err(|e| format!("Não foi possível preparar o stream: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let result = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_owned)
        .ok_or_else(|| "yt-dlp não retornou uma URL de áudio válida.".to_owned());
    eprintln!(
        "playback media-url-extraction: {} ms",
        started.elapsed().as_millis()
    );
    result
}

pub(crate) fn recommend_tracks(
    seed_id: &TrackId,
    seed_title: &str,
    seed_artist: &str,
    seed_duration_micros: i64,
    exclude: &[TrackId],
) -> Result<Vec<Track>, String> {
    match seed_id {
        TrackId::Youtube(video_id) => recommend_youtube_tracks(
            video_id,
            seed_id,
            seed_title,
            seed_artist,
            seed_duration_micros,
            exclude,
        ),
        TrackId::Audius(track_id) => recommend_audius_tracks(
            track_id,
            seed_id,
            seed_title,
            seed_artist,
            seed_duration_micros,
            exclude,
        ),
        TrackId::Spotify(sp_key) => {
            if let Some(yt_id) = crate::library::lookup_resolved_youtube_id(sp_key) {
                return recommend_youtube_tracks(
                    &yt_id,
                    seed_id,
                    seed_title,
                    seed_artist,
                    seed_duration_micros,
                    exclude,
                );
            }
            if let Ok(yt_id) =
                resolve_spotify_to_youtube(seed_title, seed_artist, seed_duration_micros)
            {
                crate::library::record_resolved_youtube_id(sp_key, &yt_id);
                return recommend_youtube_tracks(
                    &yt_id,
                    seed_id,
                    seed_title,
                    seed_artist,
                    seed_duration_micros,
                    exclude,
                );
            }
            let fallback_query = if seed_artist.trim().is_empty() {
                format!("{seed_title} mix")
            } else {
                format!("{seed_artist} {seed_title} mix")
            };
            let candidates = youtube_tracks(fallback_query.trim())?;
            Ok(curate_recommendations(
                seed_id,
                seed_title,
                seed_artist,
                seed_duration_micros,
                candidates,
                exclude,
                DEFAULT_RECOMMENDATION_LIMIT,
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) enum TrackId {
    Audius(String),
    Youtube(String),
    Spotify(String),
}
impl TryFrom<String> for TrackId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if let Some(id) = value.strip_prefix("yt:").filter(|id| youtube_id(id)) {
            Ok(Self::Youtube(id.into()))
        } else if let Some(id) = value.strip_prefix("sp:").filter(|id| spotify_track_key(id)) {
            Ok(Self::Spotify(id.into()))
        } else if is_valid_id(&value) {
            Ok(Self::Audius(value))
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
            TrackId::Spotify(id) => format!("sp:{id}"),
        }
    }
}
impl TrackId {
    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self.same_identity_with(other, crate::library::lookup_resolved_youtube_id)
    }

    fn same_identity_with(
        &self,
        other: &Self,
        resolve: impl FnOnce(&str) -> Option<String>,
    ) -> bool {
        if self == other {
            return true;
        }
        match (self, other) {
            (Self::Spotify(sp_key), Self::Youtube(yt_id))
            | (Self::Youtube(yt_id), Self::Spotify(sp_key)) => {
                resolve(sp_key).as_deref() == Some(yt_id.as_str())
            }
            _ => false,
        }
    }

    pub fn stream_url(&self) -> String {
        match self {
            Self::Youtube(id) => format!("https://www.youtube.com/watch?v={id}"),
            Self::Audius(id) => format!("{API_BASE}/tracks/{id}/stream?app_name={APP_NAME}"),
            Self::Spotify(id) => {
                if is_spotify_base62_id(id) {
                    format!("https://open.spotify.com/track/{id}")
                } else {
                    String::new()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_a_running_subprocess() {
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let signal = cancelled.clone();
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            signal.store(true, Ordering::Release);
        });
        let mut command = Command::new("sleep");
        command.arg("10");
        let started = std::time::Instant::now();

        assert!(run_command_cancellable(&mut command, Some(&cancelled)).is_err());
        cancel_thread.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn spotify_identity_survives_youtube_playback_resolution() {
        let spotify = TrackId::Spotify("source-key".into());
        let youtube = TrackId::Youtube("abcdefghijk".into());
        let resolve = |key: &str| {
            assert_eq!(key, "source-key");
            Some("abcdefghijk".to_owned())
        };
        assert!(spotify.same_identity_with(&spotify, |_| panic!("ID already matches")));
        assert!(spotify.same_identity_with(&youtube, resolve));
        assert!(youtube.same_identity_with(&spotify, resolve));
        assert!(!spotify.same_identity_with(&youtube, |_| None));
        assert!(!spotify.same_identity_with(&TrackId::Youtube("other-id123".into()), resolve,));
        assert!(
            !youtube.same_identity_with(&TrackId::Audius("abcdefghijk".into()), |_| panic!(
                "Different catalogs"
            ),)
        );
    }

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

    #[test]
    fn artwork_url_resolves_audius_and_youtube_covers() {
        let audius_raw = r#"{
            "id": "abc",
            "title": "Faixa Audius",
            "artwork": {
                "150x150": "https://cdn.example.com/small.jpg",
                "480x480": "https://cdn.example.com/medium.jpg",
                "1000x1000": "https://cdn.example.com/large.jpg"
            }
        }"#;
        let audius: Track = serde_json::from_str(audius_raw).unwrap();
        assert_eq!(
            audius.artwork_url().as_deref(),
            Some("https://cdn.example.com/medium.jpg")
        );

        let fallback_raw = r#"{
            "id": "abc",
            "title": "Fallback",
            "artwork": {
                "150x150": "https://cdn.example.com/small.jpg",
                "480x480": "   "
            }
        }"#;
        let fallback: Track = serde_json::from_str(fallback_raw).unwrap();
        assert_eq!(
            fallback.artwork_url().as_deref(),
            Some("https://cdn.example.com/small.jpg")
        );

        let youtube_raw = r#"{"id":"yt:abcdefghijk","title":"Faixa YouTube"}"#;
        let youtube: Track = serde_json::from_str(youtube_raw).unwrap();
        assert_eq!(
            youtube.artwork_url().as_deref(),
            Some("https://i.ytimg.com/vi/abcdefghijk/mqdefault.jpg")
        );

        let no_art_raw = r#"{"id":"abc","title":"Sem capa"}"#;
        let no_art: Track = serde_json::from_str(no_art_raw).unwrap();
        assert_eq!(no_art.artwork_url(), None);
    }

    fn yt_track(id: &str, title: &str, artist: &str, duration: f64) -> Track {
        Track {
            id: TrackId::Youtube(id.to_owned()),
            name: title.to_owned(),
            user: User {
                name: artist.to_owned(),
            },
            genre: "YouTube".into(),
            mood: None,
            tags: None,
            bpm: None,
            duration,
            permalink: String::new(),
            artwork: None,
            streamable: true,
            stream_gated: false,
        }
    }

    #[test]
    fn combined_search_merges_duplicate_titles_across_catalogs() {
        let youtube = yt_track(
            "abcdefghijk",
            "Numb (Official Audio)",
            "Linkin Park - Topic",
            210.0,
        );
        let same_song = Track::from_spotify(
            None,
            "Numb".into(),
            "Linkin Park".into(),
            String::new(),
            210_000,
            None,
        );
        let unique_song = Track::from_spotify(
            None,
            "In the End".into(),
            "Linkin Park".into(),
            String::new(),
            216_000,
            None,
        );

        let merged = merge_search_results(vec![youtube], vec![same_song, unique_song]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn youtube_music_search_url_selects_section_and_encodes_query() {
        let url = youtube_music_search_url("Björk & friends", "songs").unwrap();
        let url = Url::parse(&url).unwrap();
        assert_eq!(url.host_str(), Some("music.youtube.com"));
        assert_eq!(url.path(), "/search");
        assert_eq!(
            url.query_pairs().find(|(key, _)| key == "q").unwrap().1,
            "Björk & friends"
        );
        assert_eq!(url.fragment(), Some("songs"));
    }

    #[test]
    fn youtube_music_result_keeps_track_metadata_and_video_identity() {
        let result: YoutubeResult = serde_json::from_str(
            r#"{"id":"3hfSwkmp_BM","title":"Declaração de Guerra (Bleach)","artist":"Basara, 7 Minutoz, AniRap","album":"Declaração de Guerra (Bleach)","duration":630}"#,
        )
        .unwrap();
        let track = track_from_youtube_result(result).unwrap();
        assert_eq!(track.id, TrackId::Youtube("3hfSwkmp_BM".into()));
        assert_eq!(track.artist_names(), "Basara, 7 Minutoz, AniRap");
        assert_eq!(track.album_name(), "Declaração de Guerra (Bleach)");
        assert_eq!(track.duration, 630.0);
        assert_eq!(
            track.stream_url(),
            "https://www.youtube.com/watch?v=3hfSwkmp_BM"
        );
    }

    #[test]
    fn prioritized_search_uses_music_before_video_fallbacks() {
        let song = yt_track("abcdefghijk", "Song", "Artist", 180.0);
        let mut audius = yt_track("lmnopqrstuv", "Other", "Artist", 180.0);
        audius.id = TrackId::Audius("audius123".into());
        let video = yt_track("zyxwvutsrqp", "Video", "Artist", 180.0);
        let conventional = yt_track("12345678901", "Conventional", "Artist", 180.0);

        let results = prioritized_search_results(
            vec![song.clone()],
            vec![audius],
            vec![video.clone()],
            vec![conventional.clone()],
        );
        assert_eq!(results[0].id, song.id);
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].id, TrackId::Audius("audius123".into()));

        let results = prioritized_search_results(
            Vec::new(),
            Vec::new(),
            vec![video.clone()],
            vec![video.clone(), conventional.clone()],
        );
        assert_eq!(results[0].id, video.id);
        assert_eq!(results.len(), 2);

        let results = prioritized_search_results(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![conventional.clone()],
        );
        assert_eq!(results[0].id, conventional.id);
    }

    #[test]
    fn canonical_title_and_artist_normalize_video_variants() {
        assert_eq!(
            normalize_artist("Linkin Park - Topic", "Numb"),
            "linkin park"
        );
        assert_eq!(
            normalize_artist("Queen Official", "Bohemian Rhapsody"),
            "queen"
        );
        assert_eq!(normalize_artist("TaylorSwiftVEVO", "Style"), "taylorswift");
        assert_eq!(
            normalize_artist("", "Linkin Park - Numb (Official Video)"),
            "linkin park"
        );

        let a = canonical_song_title("Linkin Park - Numb (Official Video)", "Linkin Park");
        let b = canonical_song_title("Numb [4K Remaster] – Linkin Park", "Linkin Park Official");
        let c = canonical_song_title(
            "Numb (Official Music Video) [4K UPGRADE] – Linkin Park",
            "Linkin Park - Topic",
        );
        assert_eq!(a, "numb");
        assert_eq!(b, "numb");
        assert_eq!(c, "numb");

        assert_eq!(
            canonical_song_title(
                "Evanescence - Bring Me To Life (Official HD Music Video) ft. Paul McCoy",
                "Evanescence"
            ),
            "bring me to life"
        );
        assert_eq!(
            canonical_song_title("Paramore: Decode [OFFICIAL VIDEO]", "Paramore"),
            "decode"
        );
    }

    #[test]
    fn curate_recommendations_deduplicates_filters_and_interleaves_artists() {
        let seed_id = TrackId::Youtube("kXYiU_JCYtU".into());
        let excluded_id = TrackId::Youtube("excluded001".into());
        let candidates = vec![
            // Seed itself
            yt_track("kXYiU_JCYtU", "Numb (Official Video)", "Linkin Park", 188.0),
            // Duplicate of seed with different formatting / uploader
            yt_track(
                "dupNumb0001",
                "Numb [4K Remaster] – Linkin Park",
                "Reuploader",
                190.0,
            ),
            // Explicitly excluded
            yt_track("excluded001", "Already Played", "Muse", 210.0),
            // Too short (< 40s) and too long (> 900s) and non-music compilation
            yt_track("short000001", "Intro Clip", "Someone", 25.0),
            yt_track("long0000001", "Full Mixtape", "DJ Mix", 1800.0),
            yt_track(
                "album000001",
                "Hybrid Theory Full Album",
                "Linkin Park",
                400.0,
            ),
            // Same artist streak at front (should be interleaved so first pick is another artist)
            yt_track("lp_in_the_en", "In The End", "Linkin Park", 219.0),
            yt_track("lp_crawling1", "Crawling", "Linkin Park", 217.0),
            yt_track("lp_faint0001", "Faint", "Linkin Park", 169.0),
            yt_track("lp_new_divid", "New Divide", "Linkin Park", 269.0),
            // Duplicate song title within candidates
            yt_track(
                "lp_in_end_2",
                "In The End (Live)",
                "Linkin Park Official",
                230.0,
            ),
            // Related artists
            yt_track(
                "evan_bring01",
                "Evanescence - Bring Me To Life ft. Paul McCoy",
                "Evanescence",
                254.0,
            ),
            yt_track(
                "paramore_dec",
                "Paramore: Decode [OFFICIAL VIDEO]",
                "Paramore",
                254.0,
            ),
            yt_track(
                "evan_going01",
                "Evanescence - Going Under",
                "EvanescenceVEVO",
                219.0,
            ),
            yt_track("eminem_lose1", "Eminem - Lose Yourself", "Eminem", 328.0),
        ];

        let curated = curate_recommendations(
            &seed_id,
            "Numb (Official Music Video) [4K UPGRADE] – Linkin Park",
            "Linkin Park",
            188_000_000,
            candidates.clone(),
            &[excluded_id.clone()],
            7,
        );

        let ids: Vec<&TrackId> = curated.iter().map(|t| &t.id).collect();
        assert!(!ids.contains(&&seed_id));
        assert!(!ids.contains(&&excluded_id));
        assert!(!ids.contains(&&TrackId::Youtube("dupNumb0001".into())));
        assert!(!ids.contains(&&TrackId::Youtube("short000001".into())));
        assert!(!ids.contains(&&TrackId::Youtube("long0000001".into())));
        assert!(!ids.contains(&&TrackId::Youtube("album000001".into())));

        // First suggestion after seed should be from a related artist (not Linkin Park)
        assert_eq!(
            normalize_artist(curated[0].artist_names(), &curated[0].name),
            "evanescence"
        );

        // No two consecutive tracks should repeat the same artist when multiple artists exist
        for pair in curated.windows(2) {
            let a = normalize_artist(pair[0].artist_names(), &pair[0].name);
            let b = normalize_artist(pair[1].artist_names(), &pair[1].name);
            assert_ne!(a, b, "consecutive tracks shared artist {a}");
        }

        // Duplicate "In The End (Live)" was dropped; seed artist capped at 3 in pass 1
        let lp_count = curated
            .iter()
            .filter(|t| normalize_artist(t.artist_names(), &t.name) == "linkin park")
            .count();
        assert_eq!(lp_count, 3);
        assert_eq!(curated.len(), 7);

        // Pass 2 relaxes per-artist caps when total unique candidates < limit
        let filled = curate_recommendations(
            &seed_id,
            "Numb (Official Music Video) [4K UPGRADE] – Linkin Park",
            "Linkin Park",
            188_000_000,
            candidates,
            &[excluded_id],
            15,
        );
        assert_eq!(filled.len(), 8);
    }

    #[test]
    fn audius_scoring_ranks_matching_tags_genre_mood_and_bpm_higher() {
        let strong_match = Track {
            id: TrackId::Audius("match1".into()),
            name: "Midnight Rain".into(),
            user: User {
                name: "Chill Studio".into(),
            },
            genre: "Hip-Hop/Rap".into(),
            mood: Some("Peaceful".into()),
            tags: Some("lofi,chillhop,jazzhop".into()),
            bpm: Some(144.0),
            duration: 130.0,
            permalink: String::new(),
            artwork: None,
            streamable: true,
            stream_gated: false,
        };
        let weak_match = Track {
            id: TrackId::Audius("weak1".into()),
            name: "Loud Club".into(),
            user: User {
                name: "Club DJ".into(),
            },
            genre: "Electronic".into(),
            mood: Some("Aggressive".into()),
            tags: Some("edm,club".into()),
            bpm: Some(90.0),
            duration: 240.0,
            permalink: String::new(),
            artwork: None,
            streamable: true,
            stream_gated: false,
        };
        let seed_tags: HashSet<String> = ["lofi", "chillhop", "jazzhop"]
            .into_iter()
            .map(str::to_owned)
            .collect();

        let strong_score = score_audius_candidate(
            &strong_match,
            &seed_tags,
            "Hip-Hop/Rap",
            "Peaceful",
            "lofi beats",
            Some(146.0),
            120.0,
        );
        let weak_score = score_audius_candidate(
            &weak_match,
            &seed_tags,
            "Hip-Hop/Rap",
            "Peaceful",
            "lofi beats",
            Some(146.0),
            120.0,
        );
        assert!(strong_score > weak_score);
        // 3 tags (90) + genre (25) + mood (20) + bpm (12) + duration (8) = 155
        assert_eq!(strong_score, 155);
        assert_eq!(weak_score, 0);
    }

    #[test]
    fn spotify_track_id_roundtrips_and_preserves_metadata() {
        let parsed: TrackId = "sp:4HCOHRCSQelz2GPX8iTamO".to_owned().try_into().unwrap();
        assert_eq!(
            parsed,
            TrackId::Spotify("4HCOHRCSQelz2GPX8iTamO".to_owned())
        );
        assert_eq!(String::from(parsed.clone()), "sp:4HCOHRCSQelz2GPX8iTamO");
        assert_eq!(
            parsed.stream_url(),
            "https://open.spotify.com/track/4HCOHRCSQelz2GPX8iTamO"
        );
        assert!(serde_json::from_str::<TrackId>(r#""sp:../bad""#).is_err());

        let track = Track::from_spotify(
            Some("spotify:track:2iblMMIgSznA464mNov7A8"),
            "Don't Stop the Party".to_owned(),
            "Pitbull, TJR".to_owned(),
            "Global Warming".to_owned(),
            206_120,
            Some("https://image-cdn-ak.spotifycdn.com/image/cover.jpg".to_owned()),
        );
        assert_eq!(
            track.permalink(),
            "https://open.spotify.com/track/2iblMMIgSznA464mNov7A8"
        );
        assert_eq!(
            track.artwork_url().as_deref(),
            Some("https://image-cdn-ak.spotifycdn.com/image/cover.jpg")
        );
        assert_eq!(track.album_name(), "Global Warming");
        assert_eq!(track.duration_micros(), 206_120_000);

        let local_track = Track::from_spotify(
            None,
            "Custom Song".to_owned(),
            "Custom Artist".to_owned(),
            String::new(),
            120_000,
            None,
        );
        assert!(matches!(local_track.id, TrackId::Spotify(ref k) if k.starts_with("local_")));
        assert_eq!(local_track.permalink(), "");
        assert_eq!(local_track.album_name(), "Spotify");
    }

    #[test]
    fn select_best_youtube_match_prefers_studio_duration_and_artist_match() {
        let candidates = vec![
            yt_track(
                "live0000001",
                "Linkin Park - Numb (Live in Texas)",
                "Random Channel",
                260.0,
            ),
            yt_track("best0000001", "Numb", "Linkin Park - Topic", 187.0),
            yt_track("cover000001", "Numb (Piano Cover)", "Piano Covers", 190.0),
        ];

        let chosen = select_best_youtube_match(candidates, "Numb", "Linkin Park", 188_000_000);
        assert_eq!(chosen.as_deref(), Some("best0000001"));
    }
}
