use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use url::Url;

use crate::catalog::Track;
use crate::library::{SavedPlaylist, merge_liked_tracks, merge_playlists};

use super::{
    http_agent, is_connected, is_valid_id, is_valid_track_uri, playlist_meta, playlist_tracks,
    playlists, saved_tracks,
};

pub(crate) struct ImportOutcome {
    pub playlists: Vec<SavedPlaylist>,
    pub liked_tracks: Vec<Track>,
    pub summary: String,
}

impl ImportOutcome {
    fn empty() -> Self {
        Self {
            playlists: Vec::new(),
            liked_tracks: Vec::new(),
            summary: String::new(),
        }
    }

    fn merge_from(&mut self, other: ImportOutcome) {
        merge_playlists(&mut self.playlists, other.playlists);
        merge_liked_tracks(&mut self.liked_tracks, other.liked_tracks);
    }

    fn is_empty(&self) -> bool {
        self.playlists.is_empty() && self.liked_tracks.is_empty()
    }

    fn finalize(mut self) -> Self {
        let total_playlist_tracks: usize = self.playlists.iter().map(|p| p.tracks.len()).sum();
        self.summary = match (self.playlists.len(), self.liked_tracks.len()) {
            (0, 0) => "Nenhuma música encontrada para importar.".to_owned(),
            (1, 0) => format!(
                "Playlist \"{}\" pronta ({} faixas).",
                self.playlists[0].name,
                self.playlists[0].tracks.len()
            ),
            (p, 0) => {
                format!("{p} playlists importadas ({total_playlist_tracks} faixas no total).")
            }
            (0, l) => format!("{l} músicas curtidas importadas."),
            (p, l) => format!(
                "{p} playlists ({total_playlist_tracks} faixas) e {l} músicas curtidas importadas."
            ),
        };
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpotifyTargetKind {
    Playlist,
    Album,
    Track,
}

impl SpotifyTargetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Playlist => "playlist",
            Self::Album => "album",
            Self::Track => "track",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpotifyTarget {
    kind: SpotifyTargetKind,
    id: String,
}

/// Retorna `true` quando o texto contém exclusivamente links ou URIs válidos do Spotify.
pub(crate) fn is_spotify_url_or_uri(input: &str) -> bool {
    let tokens: Vec<&str> = input
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|token| token.trim_matches(|c| matches!(c, '<' | '>' | '"' | '\'')))
        .filter(|token| !token.is_empty())
        .collect();
    !tokens.is_empty() && tokens.iter().all(|token| parse_target(token).is_some())
}

fn parse_target(token: &str) -> Option<SpotifyTarget> {
    let token = token.trim_matches(|c| matches!(c, '<' | '>' | '"' | '\''));
    if let Some(rest) = token.strip_prefix("spotify:") {
        let mut parts = rest.split(':');
        let kind = match parts.next()? {
            "playlist" => SpotifyTargetKind::Playlist,
            "album" => SpotifyTargetKind::Album,
            "track" => SpotifyTargetKind::Track,
            _ => return None,
        };
        let raw_id = parts.next()?.split('?').next()?;
        if parts.next().is_some() || !is_valid_id(raw_id) {
            return None;
        }
        return Some(SpotifyTarget {
            kind,
            id: raw_id.to_owned(),
        });
    }

    let normalized = if token.starts_with("open.spotify.com/") {
        format!("https://{token}")
    } else {
        token.to_owned()
    };
    let url = Url::parse(&normalized).ok()?;
    let host = url.host_str()?;
    if host != "open.spotify.com" && host != "play.spotify.com" {
        return None;
    }

    let segments: Vec<&str> = url
        .path_segments()?
        .filter(|seg| !seg.is_empty())
        .filter(|seg| *seg != "embed" && !seg.starts_with("intl-"))
        .collect();
    if segments.len() < 2 {
        return None;
    }
    let kind = match segments[0] {
        "playlist" => SpotifyTargetKind::Playlist,
        "album" => SpotifyTargetKind::Album,
        "track" => SpotifyTargetKind::Track,
        _ => return None,
    };
    let id = segments[1];
    is_valid_id(id).then(|| SpotifyTarget {
        kind,
        id: id.to_owned(),
    })
}

fn parse_targets(input: &str) -> Vec<SpotifyTarget> {
    input
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(parse_target)
        .collect()
}

/// Importa uma ou mais playlists, álbuns ou faixas a partir de URLs/URIs do Spotify.
pub(crate) fn import_url_or_uri(
    input: &str,
    client_id: Option<&str>,
) -> Result<ImportOutcome, String> {
    let targets = parse_targets(input);
    if targets.is_empty() {
        return Err(
            "Cole um link ou URI válido do Spotify (ex.: https://open.spotify.com/playlist/... ou spotify:playlist:...)."
                .to_owned(),
        );
    }

    let active_client_id = client_id
        .map(str::trim)
        .filter(|cid| !cid.is_empty() && is_connected(cid));

    let mut outcome = ImportOutcome::empty();
    for target in targets {
        match target.kind {
            SpotifyTargetKind::Playlist => {
                let playlist = if let Some(cid) = active_client_id {
                    match fetch_playlist_via_api(cid, &target.id) {
                        Ok(playlist) if !playlist.tracks.is_empty() => playlist,
                        _ => fetch_public_embed(SpotifyTargetKind::Playlist, &target.id)?
                            .playlists
                            .into_iter()
                            .next()
                            .ok_or_else(|| "Playlist vazia no Spotify.".to_owned())?,
                    }
                } else {
                    fetch_public_embed(SpotifyTargetKind::Playlist, &target.id)?
                        .playlists
                        .into_iter()
                        .next()
                        .ok_or_else(|| "Playlist vazia no Spotify.".to_owned())?
                };
                merge_playlists(&mut outcome.playlists, vec![playlist]);
            }
            SpotifyTargetKind::Album | SpotifyTargetKind::Track => {
                let part = fetch_public_embed(target.kind, &target.id)?;
                outcome.merge_from(part);
            }
        }
    }

    if outcome.is_empty() {
        return Err("Nenhuma faixa encontrada no link informado.".to_owned());
    }
    Ok(outcome.finalize())
}

fn fetch_playlist_via_api(client_id: &str, playlist_id: &str) -> Result<SavedPlaylist, String> {
    let meta = playlist_meta(client_id, playlist_id)?;
    let mut tracks = Vec::new();
    let mut offset = 0;
    loop {
        let page = playlist_tracks(client_id, playlist_id, offset)?;
        tracks.extend(page.items.into_iter().map(super::Track::into_catalog_track));
        let Some(next) = page.next_offset else {
            break;
        };
        offset = next;
    }
    let artwork_url = meta.artwork_url();
    Ok(SavedPlaylist {
        id: format!("spotify:playlist:{playlist_id}"),
        name: if meta.name.trim().is_empty() {
            "Playlist do Spotify".to_owned()
        } else {
            meta.name
        },
        description: meta.description.unwrap_or_default(),
        artwork_url,
        source_url: Some(format!("https://open.spotify.com/playlist/{playlist_id}")),
        tracks,
    })
}

fn fetch_public_embed(kind: SpotifyTargetKind, id: &str) -> Result<ImportOutcome, String> {
    let url = format!("https://open.spotify.com/embed/{}/{id}", kind.as_str());
    let mut response = http_agent()
        .get(&url)
        .header("Accept", "text/html,application/xhtml+xml")
        .header("User-Agent", "Mozilla/5.0")
        .call()
        .map_err(|error| format!("Não foi possível acessar o Spotify: {error}"))?;

    let status = response.status().as_u16();
    if status == 404 {
        return Err("Item não encontrado ou privado no Spotify.".to_owned());
    }
    if status >= 400 {
        return Err(format!("O Spotify respondeu com HTTP {status}."));
    }

    let html = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("Não foi possível ler a página do Spotify: {error}"))?;
    parse_embed_html(&html, kind, id)
}

fn parse_embed_html(
    html: &str,
    kind: SpotifyTargetKind,
    id: &str,
) -> Result<ImportOutcome, String> {
    let marker = r#"id="__NEXT_DATA__""#;
    let start_tag = html
        .find(marker)
        .ok_or_else(|| "O Spotify não retornou dados públicos para este link.".to_owned())?;
    let after_tag = &html[start_tag..];
    let json_start = after_tag
        .find('>')
        .map(|pos| pos + 1)
        .ok_or_else(|| "Formato inesperado na página do Spotify.".to_owned())?;
    let json_slice = &after_tag[json_start..];
    let json_end = json_slice
        .find("</script>")
        .ok_or_else(|| "Dados da página do Spotify incompletos.".to_owned())?;
    let next_data: EmbedNextData = serde_json::from_str(&json_slice[..json_end])
        .map_err(|error| format!("Falha ao interpretar metadados do Spotify: {error}"))?;

    let entity = next_data.props.page_props.state.data.entity;
    let artwork_url = entity.artwork_url();
    let title = clean_embed_text(if entity.title.trim().is_empty() {
        &entity.name
    } else {
        &entity.title
    });
    let subtitle = clean_embed_text(&entity.subtitle);

    let mut outcome = ImportOutcome::empty();
    match kind {
        SpotifyTargetKind::Playlist | SpotifyTargetKind::Album => {
            let playlist_name = if title.is_empty() {
                if kind == SpotifyTargetKind::Album {
                    "Álbum do Spotify".to_owned()
                } else {
                    "Playlist do Spotify".to_owned()
                }
            } else {
                title
            };
            let album_name = if kind == SpotifyTargetKind::Album {
                playlist_name.clone()
            } else {
                String::new()
            };
            let description = if subtitle.is_empty() {
                String::new()
            } else if kind == SpotifyTargetKind::Album {
                format!("Álbum · {subtitle}")
            } else {
                format!("Por {subtitle}")
            };

            let tracks: Vec<Track> = entity
                .track_list
                .into_iter()
                .filter(|item| {
                    item.entity_type.is_empty() || item.entity_type.eq_ignore_ascii_case("track")
                })
                .filter(|item| !item.title.trim().is_empty())
                .map(|item| {
                    Track::from_spotify(
                        Some(&item.uri),
                        clean_embed_text(&item.title),
                        clean_embed_text(&item.subtitle),
                        album_name.clone(),
                        item.duration,
                        artwork_url.clone(),
                    )
                })
                .collect();

            if tracks.is_empty() {
                return Err(
                    "Esta playlist ou álbum não possui faixas públicas disponíveis.".to_owned(),
                );
            }

            outcome.playlists.push(SavedPlaylist {
                id: format!("spotify:{}:{id}", kind.as_str()),
                name: playlist_name,
                description,
                artwork_url,
                source_url: Some(format!("https://open.spotify.com/{}/{id}", kind.as_str())),
                tracks,
            });
        }
        SpotifyTargetKind::Track => {
            if title.is_empty() {
                return Err("Faixa inválida retornada pelo Spotify.".to_owned());
            }
            let artists = if entity.artists.is_empty() {
                subtitle
            } else {
                entity
                    .artists
                    .into_iter()
                    .map(|artist| clean_embed_text(&artist.name))
                    .filter(|name| !name.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let uri = if entity.uri.trim().is_empty() {
                format!("spotify:track:{id}")
            } else {
                entity.uri
            };
            outcome.liked_tracks.push(Track::from_spotify(
                Some(&uri),
                title,
                artists,
                String::new(),
                entity.duration,
                artwork_url,
            ));
        }
    }

    Ok(outcome.finalize())
}

fn clean_embed_text(raw: &str) -> String {
    raw.replace('\u{00a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Conecta à conta do Spotify via OAuth PKCE e importa músicas curtidas e playlists.
pub(crate) fn sync_account(client_id: &str) -> Result<ImportOutcome, String> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err("Informe e salve seu Client ID do Spotify antes de conectar.".to_owned());
    }

    super::connect(client_id)?;

    let mut outcome = ImportOutcome::empty();
    let mut offset = 0;
    loop {
        let page = saved_tracks(client_id, offset)?;
        outcome
            .liked_tracks
            .extend(page.items.into_iter().map(super::Track::into_catalog_track));
        let Some(next) = page.next_offset else {
            break;
        };
        offset = next;
    }

    let mut pl_offset = 0;
    loop {
        let page = playlists(client_id, pl_offset)?;
        for pl in page.items {
            let mut tracks = Vec::new();
            let mut tr_offset = 0;
            loop {
                let tr_page = playlist_tracks(client_id, &pl.id, tr_offset)?;
                tracks.extend(
                    tr_page
                        .items
                        .into_iter()
                        .map(super::Track::into_catalog_track),
                );
                let Some(next) = tr_page.next_offset else {
                    break;
                };
                tr_offset = next;
            }
            outcome.playlists.push(SavedPlaylist {
                id: format!("spotify:playlist:{}", pl.id),
                name: if pl.name.trim().is_empty() {
                    "Playlist do Spotify".to_owned()
                } else {
                    pl.name.clone()
                },
                description: pl.description.clone().unwrap_or_default(),
                artwork_url: pl.artwork_url(),
                source_url: Some(format!("https://open.spotify.com/playlist/{}", pl.id)),
                tracks,
            });
        }
        let Some(next) = page.next_offset else {
            break;
        };
        pl_offset = next;
    }

    Ok(outcome.finalize())
}

/// Importa um arquivo JSON/CSV ou uma pasta inteira de exportação de dados do Spotify.
pub(crate) fn import_path(raw_path: &str) -> Result<ImportOutcome, String> {
    let trimmed = raw_path.trim().trim_matches(|c| matches!(c, '"' | '\''));
    if trimmed.is_empty() {
        return Err("Informe o caminho de um arquivo JSON/CSV ou pasta do Spotify.".to_owned());
    }

    let path = expand_home(trimmed);
    if !path.exists() {
        return Err(format!("Caminho não encontrado: {}", path.display()));
    }

    if path.is_dir() {
        let mut files = Vec::new();
        collect_export_files(&path, 0, &mut files);
        files.sort();
        let mut outcome = ImportOutcome::empty();
        for file in files {
            if let Ok(part) = import_file(&file, false) {
                outcome.merge_from(part);
            }
        }
        if outcome.is_empty() {
            return Err(
                "Nenhum arquivo de playlist ou biblioteca do Spotify encontrado nessa pasta."
                    .to_owned(),
            );
        }
        return Ok(outcome.finalize());
    }

    let outcome = import_file(&path, true)?;
    if outcome.is_empty() {
        return Err("O arquivo não contém playlists ou músicas importáveis.".to_owned());
    }
    Ok(outcome.finalize())
}

/// Procura automaticamente exportações do Spotify em `~/Downloads`.
pub(crate) fn scan_downloads() -> Result<ImportOutcome, String> {
    let downloads = downloads_dir()
        .ok_or_else(|| "Não foi possível localizar a pasta Downloads.".to_owned())?;
    if !downloads.is_dir() {
        return Err(format!(
            "Pasta Downloads não encontrada em {}.",
            downloads.display()
        ));
    }

    let entries = fs::read_dir(&downloads)
        .map_err(|error| format!("Não foi possível ler {}: {error}", downloads.display()))?;
    let mut candidate_paths = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name_lower = entry.file_name().to_string_lossy().to_lowercase();
        if name_lower.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if name_lower.contains("spotify") || name_lower == "mydata" {
                candidate_paths.push(path);
            }
        } else if is_candidate_download_file(&path, &name_lower) {
            candidate_paths.push(path);
        }
    }
    candidate_paths.sort();

    let mut outcome = ImportOutcome::empty();
    for path in candidate_paths {
        if let Ok(part) = import_path(&path.to_string_lossy()) {
            outcome.merge_from(part);
        }
    }
    if outcome.is_empty() {
        return Err(
            "Nenhum arquivo de exportação do Spotify (Playlist*.json, YourLibrary.json, StreamingHistory*.json ou CSV do Exportify) encontrado em Downloads."
                .to_owned(),
        );
    }
    Ok(outcome.finalize())
}

fn is_candidate_download_file(path: &Path, name_lower: &str) -> bool {
    if name_lower.ends_with(".json") {
        return name_lower.starts_with("playlist")
            || name_lower == "yourlibrary.json"
            || name_lower.starts_with("streaminghistory")
            || name_lower.starts_with("streaming_history");
    }
    if name_lower.ends_with(".csv")
        && let Ok(text) = fs::read_to_string(path)
    {
        let first_line = text.lines().next().unwrap_or_default().to_lowercase();
        return first_line.contains("track uri")
            || first_line.contains("track name")
            || first_line.contains("spotify uri");
    }
    false
}

fn downloads_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("XDG_DOWNLOAD_DIR") {
        let p = PathBuf::from(dir.trim());
        if p.is_absolute() {
            return Some(p);
        }
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join("Downloads"))
}

fn expand_home(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(raw)
}

fn collect_export_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 2 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_export_files(&path, depth + 1, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && matches!(ext.to_ascii_lowercase().as_str(), "json" | "csv")
        {
            out.push(path);
        }
    }
}

fn import_file(path: &Path, strict: bool) -> Result<ImportOutcome, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let bytes = fs::read(path)
        .map_err(|error| format!("Não foi possível ler {}: {error}", path.display()))?;
    match ext.as_str() {
        "json" => parse_json_bytes(&bytes, strict),
        "csv" => {
            let text = String::from_utf8_lossy(&bytes);
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Playlist Importada");
            parse_csv_str(&text, stem, strict)
        }
        _ => {
            if strict {
                Err(
                    "Formato não suportado. Use arquivos .json ou .csv exportados do Spotify."
                        .to_owned(),
                )
            } else {
                Ok(ImportOutcome::empty())
            }
        }
    }
}

fn parse_json_bytes(bytes: &[u8], strict: bool) -> Result<ImportOutcome, String> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| format!("Arquivo JSON inválido: {error}"))?;

    let mut outcome = ImportOutcome::empty();
    match &value {
        serde_json::Value::Object(map) => {
            if map
                .get("playlists")
                .is_some_and(serde_json::Value::is_array)
            {
                let parsed: PrivacyPlaylistsFile = serde_json::from_value(value)
                    .map_err(|error| format!("Formato Playlist.json inválido: {error}"))?;
                for pl in parsed.playlists {
                    if let Some(saved) = privacy_playlist_to_saved(pl) {
                        merge_playlists(&mut outcome.playlists, vec![saved]);
                    }
                }
                return Ok(outcome);
            }
            if map.get("tracks").is_some_and(serde_json::Value::is_array) {
                let parsed: YourLibraryFile = serde_json::from_value(value)
                    .map_err(|error| format!("Formato YourLibrary.json inválido: {error}"))?;
                let tracks: Vec<Track> = parsed
                    .tracks
                    .into_iter()
                    .filter(|t| !t.track.trim().is_empty())
                    .map(|t| Track::from_spotify(Some(&t.uri), t.track, t.artist, t.album, 0, None))
                    .collect();
                merge_liked_tracks(&mut outcome.liked_tracks, tracks);
                return Ok(outcome);
            }
            if map
                .get("tracks")
                .and_then(|t| t.get("items"))
                .is_some_and(serde_json::Value::is_array)
            {
                let parsed: WebApiPlaylistJson = serde_json::from_value(value)
                    .map_err(|error| format!("Playlist JSON da API inválido: {error}"))?;
                let pl_id = if is_valid_id(&parsed.id) {
                    format!("spotify:playlist:{}", parsed.id)
                } else {
                    format!("spotify:import:{}", slug(&parsed.name))
                };
                let source_url = is_valid_id(&parsed.id)
                    .then(|| format!("https://open.spotify.com/playlist/{}", parsed.id));
                let tracks: Vec<Track> = parsed
                    .tracks
                    .items
                    .into_iter()
                    .filter_map(|item| item.item)
                    .filter(|t| is_valid_track_uri(&t.uri) || !t.name.trim().is_empty())
                    .map(super::Track::into_catalog_track)
                    .collect();
                if !tracks.is_empty() {
                    outcome.playlists.push(SavedPlaylist {
                        id: pl_id,
                        name: if parsed.name.trim().is_empty() {
                            "Playlist do Spotify".to_owned()
                        } else {
                            parsed.name
                        },
                        description: parsed.description.unwrap_or_default(),
                        artwork_url: None,
                        source_url,
                        tracks,
                    });
                }
                return Ok(outcome);
            }
            if map.get("items").is_some_and(serde_json::Value::is_array) {
                let parsed: WebApiSavedTracksJson = serde_json::from_value(value)
                    .map_err(|error| format!("Biblioteca JSON da API inválida: {error}"))?;
                let tracks: Vec<Track> = parsed
                    .items
                    .into_iter()
                    .filter_map(|item| item.track)
                    .filter(|t| !t.name.trim().is_empty())
                    .map(super::Track::into_catalog_track)
                    .collect();
                merge_liked_tracks(&mut outcome.liked_tracks, tracks);
                return Ok(outcome);
            }
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return Ok(outcome);
            }
            let first = &items[0];
            if first.get("trackName").is_some() || first.get("master_metadata_track_name").is_some()
            {
                let entries: Vec<StreamingHistoryEntry> = serde_json::from_value(value)
                    .map_err(|error| format!("Formato StreamingHistory inválido: {error}"))?;
                let mut tracks = Vec::new();
                for entry in entries {
                    let title = entry
                        .track_name
                        .or(entry.master_metadata_track_name)
                        .unwrap_or_default();
                    let artist = entry
                        .artist_name
                        .or(entry.master_metadata_album_artist_name)
                        .unwrap_or_default();
                    if title.trim().is_empty() {
                        continue;
                    }
                    let album = entry.master_metadata_album_album_name.unwrap_or_default();
                    let uri = entry.spotify_track_uri;
                    tracks.push(Track::from_spotify(
                        uri.as_deref(),
                        title,
                        artist,
                        album,
                        0,
                        None,
                    ));
                }
                let mut deduped = Vec::new();
                merge_liked_tracks(&mut deduped, tracks);
                if !deduped.is_empty() {
                    outcome.playlists.push(SavedPlaylist {
                        id: "spotify:export:streaming-history".to_owned(),
                        name: "Histórico do Spotify".to_owned(),
                        description: "Importado do histórico de reprodução do Spotify".to_owned(),
                        artwork_url: None,
                        source_url: None,
                        tracks: deduped,
                    });
                }
                return Ok(outcome);
            }
            if first.get("items").is_some() && first.get("name").is_some() {
                let playlists: Vec<PrivacyPlaylist> = serde_json::from_value(value)
                    .map_err(|error| format!("Lista de playlists JSON inválida: {error}"))?;
                for pl in playlists {
                    if let Some(saved) = privacy_playlist_to_saved(pl) {
                        merge_playlists(&mut outcome.playlists, vec![saved]);
                    }
                }
                return Ok(outcome);
            }
            if first.get("tracks").is_some() && first.get("name").is_some() {
                let playlists: Vec<SavedPlaylist> = serde_json::from_value(value)
                    .map_err(|error| format!("Backup de playlists JSON inválido: {error}"))?;
                merge_playlists(&mut outcome.playlists, playlists);
                return Ok(outcome);
            }
            if first.get("id").is_some() && first.get("title").is_some() {
                let tracks: Vec<Track> = serde_json::from_value(value)
                    .map_err(|error| format!("Backup de faixas JSON inválido: {error}"))?;
                merge_liked_tracks(&mut outcome.liked_tracks, tracks);
                return Ok(outcome);
            }
        }
        _ => {}
    }

    if strict {
        Err(
            "JSON não reconhecido como exportação do Spotify (Playlist1.json, YourLibrary.json, StreamingHistory.json ou backup)."
                .to_owned(),
        )
    } else {
        Ok(outcome)
    }
}

fn privacy_playlist_to_saved(pl: PrivacyPlaylist) -> Option<SavedPlaylist> {
    let name = if pl.name.trim().is_empty() {
        "Playlist do Spotify".to_owned()
    } else {
        pl.name.trim().to_owned()
    };
    let tracks: Vec<Track> = pl
        .items
        .into_iter()
        .filter_map(|item| {
            if let Some(track) = item.track {
                if track.track_name.trim().is_empty() {
                    return None;
                }
                return Some(Track::from_spotify(
                    track.track_uri.as_deref(),
                    track.track_name,
                    track.artist_name,
                    track.album_name,
                    0,
                    None,
                ));
            }
            if let Some(local) = item.local_track {
                if local.track_name.trim().is_empty() {
                    return None;
                }
                return Some(Track::from_spotify(
                    local.uri.as_deref(),
                    local.track_name,
                    local.artist_name,
                    local.album_name,
                    0,
                    None,
                ));
            }
            None
        })
        .collect();
    if tracks.is_empty() {
        return None;
    }
    Some(SavedPlaylist {
        id: format!("spotify:export:{}", slug(&name)),
        name,
        description: pl.description.unwrap_or_default(),
        artwork_url: None,
        source_url: None,
        tracks,
    })
}

fn parse_csv_str(text: &str, file_stem: &str, strict: bool) -> Result<ImportOutcome, String> {
    let rows = parse_csv_records(text);
    if rows.len() < 2 {
        return if strict {
            Err("O arquivo CSV está vazio ou sem linhas de músicas.".to_owned())
        } else {
            Ok(ImportOutcome::empty())
        };
    }

    let headers: Vec<String> = rows[0].iter().map(|h| h.trim().to_lowercase()).collect();

    let find_col = |aliases: &[&str]| -> Option<usize> {
        aliases
            .iter()
            .find_map(|alias| headers.iter().position(|h| h == alias))
    };

    let uri_col = find_col(&[
        "track uri",
        "spotify uri",
        "uri",
        "track id",
        "spotify id",
        "id",
    ]);
    let title_col = find_col(&[
        "track name",
        "title",
        "name",
        "track",
        "song",
        "musica",
        "música",
    ]);
    let artist_col = find_col(&[
        "artist name(s)",
        "artist names",
        "artist name",
        "artists",
        "artist",
        "artista",
        "artistas",
    ]);
    let album_col = find_col(&["album name", "album", "álbum"]);
    let duration_col = find_col(&[
        "track duration (ms)",
        "duration (ms)",
        "duration_ms",
        "duration",
    ]);
    let artwork_col = find_col(&[
        "album image url",
        "image url",
        "artwork_url",
        "cover",
        "album art",
    ]);
    let playlist_col = find_col(&["playlist name", "playlist"]);

    let Some(title_idx) = title_col else {
        return if strict {
            Err("Coluna de título ('Track Name') não encontrada no CSV.".to_owned())
        } else {
            Ok(ImportOutcome::empty())
        };
    };

    let get = |row: &[String], idx: Option<usize>| -> String {
        idx.and_then(|i| row.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };

    let mut grouped: BTreeMap<String, Vec<Track>> = BTreeMap::new();
    let mut flat_tracks = Vec::new();

    for row in &rows[1..] {
        let title = get(row, Some(title_idx));
        if title.is_empty() {
            continue;
        }
        let uri = get(row, uri_col);
        let artist = normalize_csv_artists(&get(row, artist_col));
        let album = get(row, album_col);
        let duration_ms = parse_duration_ms(&get(row, duration_col));
        let artwork = {
            let url = get(row, artwork_col);
            (!url.is_empty()).then_some(url)
        };
        let track = Track::from_spotify(
            (!uri.is_empty()).then_some(uri.as_str()),
            title,
            artist,
            album,
            duration_ms,
            artwork,
        );

        let pl_name = get(row, playlist_col);
        if !pl_name.is_empty() {
            grouped.entry(pl_name).or_default().push(track);
        } else {
            flat_tracks.push(track);
        }
    }

    let mut outcome = ImportOutcome::empty();
    for (name, tracks) in grouped {
        merge_playlists(
            &mut outcome.playlists,
            vec![SavedPlaylist {
                id: format!("spotify:csv:{}", slug(&name)),
                name,
                description: "Importada via CSV".to_owned(),
                artwork_url: tracks.iter().find_map(Track::artwork_url),
                source_url: None,
                tracks,
            }],
        );
    }

    if !flat_tracks.is_empty() {
        let stem_lower = file_stem.trim().to_lowercase();
        if matches!(
            stem_lower.as_str(),
            "liked"
                | "liked_songs"
                | "liked songs"
                | "liked_tracks"
                | "curtidas"
                | "musicas_curtidas"
        ) {
            merge_liked_tracks(&mut outcome.liked_tracks, flat_tracks);
        } else {
            let name = file_stem.replace('_', " ").trim().to_owned();
            let name = if name.is_empty() {
                "Playlist CSV".to_owned()
            } else {
                name
            };
            let artwork_url = flat_tracks.iter().find_map(Track::artwork_url);
            merge_playlists(
                &mut outcome.playlists,
                vec![SavedPlaylist {
                    id: format!("spotify:csv:{}", slug(&name)),
                    name,
                    description: "Importada via CSV".to_owned(),
                    artwork_url,
                    source_url: None,
                    tracks: flat_tracks,
                }],
            );
        }
    }

    Ok(outcome)
}

fn normalize_csv_artists(raw: &str) -> String {
    if raw.contains(';') {
        raw.split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        raw.trim().to_owned()
    }
}

fn parse_duration_ms(raw: &str) -> u64 {
    let raw = raw.trim();
    if raw.is_empty() {
        return 0;
    }
    if let Ok(ms) = raw.parse::<u64>() {
        return ms;
    }
    if let Ok(ms) = raw.parse::<f64>()
        && ms.is_finite()
        && ms >= 0.0
    {
        return ms as u64;
    }
    if let Some((min, sec)) = raw.split_once(':')
        && let (Ok(m), Ok(s)) = (min.trim().parse::<u64>(), sec.trim().parse::<u64>())
    {
        return (m * 60 + s) * 1000;
    }
    0
}

fn parse_csv_records(raw: &str) -> Vec<Vec<String>> {
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let delimiter = detect_csv_delimiter(input);
    let mut records = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            in_quotes = true;
        } else if ch == delimiter {
            row.push(std::mem::take(&mut field));
        } else if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            if row.iter().any(|cell| !cell.trim().is_empty()) {
                records.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else if ch == '\n' {
            row.push(std::mem::take(&mut field));
            if row.iter().any(|cell| !cell.trim().is_empty()) {
                records.push(std::mem::take(&mut row));
            } else {
                row.clear();
            }
        } else {
            field.push(ch);
        }
    }

    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        if row.iter().any(|cell| !cell.trim().is_empty()) {
            records.push(row);
        }
    }
    records
}

fn detect_csv_delimiter(input: &str) -> char {
    let mut commas = 0usize;
    let mut semis = 0usize;
    let mut in_quotes = false;
    for ch in input.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            if ch == '\n' || ch == '\r' {
                break;
            }
            if ch == ',' {
                commas += 1;
            } else if ch == ';' {
                semis += 1;
            }
        }
    }
    if semis > commas { ';' } else { ',' }
}

fn slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "playlist".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[derive(Deserialize)]
struct EmbedNextData {
    props: EmbedProps,
}

#[derive(Deserialize)]
struct EmbedProps {
    #[serde(rename = "pageProps")]
    page_props: EmbedPageProps,
}

#[derive(Deserialize)]
struct EmbedPageProps {
    state: EmbedState,
}

#[derive(Deserialize)]
struct EmbedState {
    data: EmbedData,
}

#[derive(Deserialize)]
struct EmbedData {
    entity: EmbedEntity,
}

#[derive(Deserialize)]
struct EmbedEntity {
    #[serde(default)]
    name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    uri: String,
    #[serde(default)]
    duration: u64,
    #[serde(default)]
    artists: Vec<EmbedArtist>,
    #[serde(default, rename = "coverArt")]
    cover_art: Option<EmbedCoverArt>,
    #[serde(default, rename = "visualIdentity")]
    visual_identity: Option<EmbedVisualIdentity>,
    #[serde(default, rename = "trackList")]
    track_list: Vec<EmbedTrack>,
}

impl EmbedEntity {
    fn artwork_url(&self) -> Option<String> {
        if let Some(cover) = &self.cover_art {
            for src in cover.sources.iter().rev() {
                let u = src.url.trim();
                if u.starts_with("http://") || u.starts_with("https://") {
                    return Some(u.to_owned());
                }
            }
        }
        if let Some(vis) = &self.visual_identity {
            let valid = |img: &&EmbedVisualImage| {
                let u = img.url.trim();
                u.starts_with("http://") || u.starts_with("https://")
            };
            if let Some(best) = vis
                .image
                .iter()
                .filter(valid)
                .find(|img| img.max_width.is_some_and(|w| (200..=640).contains(&w)))
                .or_else(|| vis.image.iter().find(valid))
            {
                return Some(best.url.trim().to_owned());
            }
        }
        None
    }
}

#[derive(Deserialize)]
struct EmbedArtist {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct EmbedCoverArt {
    #[serde(default)]
    sources: Vec<EmbedCoverSource>,
}

#[derive(Deserialize)]
struct EmbedCoverSource {
    #[serde(default)]
    url: String,
}

#[derive(Deserialize)]
struct EmbedVisualIdentity {
    #[serde(default)]
    image: Vec<EmbedVisualImage>,
}

#[derive(Deserialize)]
struct EmbedVisualImage {
    #[serde(default)]
    url: String,
    #[serde(default, rename = "maxWidth")]
    max_width: Option<u32>,
}

#[derive(Deserialize)]
struct EmbedTrack {
    #[serde(default)]
    uri: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    duration: u64,
    #[serde(default, rename = "entityType")]
    entity_type: String,
}

#[derive(Deserialize)]
struct PrivacyPlaylistsFile {
    #[serde(default)]
    playlists: Vec<PrivacyPlaylist>,
}

#[derive(Deserialize)]
struct PrivacyPlaylist {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    items: Vec<PrivacyPlaylistItem>,
}

#[derive(Deserialize)]
struct PrivacyPlaylistItem {
    track: Option<PrivacyTrack>,
    #[serde(rename = "localTrack")]
    local_track: Option<PrivacyLocalTrack>,
}

#[derive(Deserialize)]
struct PrivacyTrack {
    #[serde(default, rename = "trackName")]
    track_name: String,
    #[serde(default, rename = "artistName")]
    artist_name: String,
    #[serde(default, rename = "albumName")]
    album_name: String,
    #[serde(default, rename = "trackUri")]
    track_uri: Option<String>,
}

#[derive(Deserialize)]
struct PrivacyLocalTrack {
    #[serde(default, rename = "trackName")]
    track_name: String,
    #[serde(default, rename = "artistName")]
    artist_name: String,
    #[serde(default, rename = "albumName")]
    album_name: String,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Deserialize)]
struct YourLibraryFile {
    #[serde(default)]
    tracks: Vec<YourLibraryTrack>,
}

#[derive(Deserialize)]
struct YourLibraryTrack {
    #[serde(default)]
    artist: String,
    #[serde(default)]
    album: String,
    #[serde(default)]
    track: String,
    #[serde(default)]
    uri: String,
}

#[derive(Deserialize)]
struct StreamingHistoryEntry {
    #[serde(rename = "trackName")]
    track_name: Option<String>,
    #[serde(rename = "artistName")]
    artist_name: Option<String>,
    master_metadata_track_name: Option<String>,
    master_metadata_album_artist_name: Option<String>,
    master_metadata_album_album_name: Option<String>,
    spotify_track_uri: Option<String>,
}

#[derive(Deserialize)]
struct WebApiPlaylistJson {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: Option<String>,
    tracks: WebApiPlaylistTracks,
}

#[derive(Deserialize)]
struct WebApiPlaylistTracks {
    #[serde(default)]
    items: Vec<WebApiPlaylistTrackItem>,
}

#[derive(Deserialize)]
struct WebApiPlaylistTrackItem {
    #[serde(default, alias = "track")]
    item: Option<super::Track>,
}

#[derive(Deserialize)]
struct WebApiSavedTracksJson {
    #[serde(default)]
    items: Vec<WebApiSavedTrackItem>,
}

#[derive(Deserialize)]
struct WebApiSavedTrackItem {
    track: Option<super::Track>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::TrackId;

    #[test]
    fn parses_spotify_urls_and_uris_including_localized_paths() {
        assert!(is_spotify_url_or_uri(
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc"
        ));
        assert!(is_spotify_url_or_uri(
            "https://open.spotify.com/intl-pt/album/4aawyAB9vmqN3uQ7FjRGTy"
        ));
        assert!(is_spotify_url_or_uri(
            "spotify:track:4HCOHRCSQelz2GPX8iTamO"
        ));
        assert!(!is_spotify_url_or_uri("spotify"));
        assert!(!is_spotify_url_or_uri("linkin park numb"));

        let parsed = parse_targets(
            "https://open.spotify.com/intl-pt/playlist/37i9dQZF1DXcBWIGoYBM5M?si=1 spotify:album:4aawyAB9vmqN3uQ7FjRGTy",
        );
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].kind, SpotifyTargetKind::Playlist);
        assert_eq!(parsed[0].id, "37i9dQZF1DXcBWIGoYBM5M");
        assert_eq!(parsed[1].kind, SpotifyTargetKind::Album);
        assert_eq!(parsed[1].id, "4aawyAB9vmqN3uQ7FjRGTy");
    }

    #[test]
    fn parses_embed_next_data_for_playlist_and_track() {
        let playlist_html = r#"<!doctype html><html><head><script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"type":"playlist","title":"Top Hits","subtitle":"Spotify","coverArt":{"sources":[{"url":"https://i.scdn.co/image/cover.jpg"}]},"trackList":[{"uri":"spotify:track:4HCOHRCSQelz2GPX8iTamO","title":"Flower In A Hurricane","subtitle":"Justice,\u00a0Tame Impala","duration":224877,"entityType":"track"}]}}}}}}</script></head></html>"#;
        let outcome = parse_embed_html(
            playlist_html,
            SpotifyTargetKind::Playlist,
            "37i9dQZF1DXcBWIGoYBM5M",
        )
        .unwrap();
        assert_eq!(outcome.playlists.len(), 1);
        let pl = &outcome.playlists[0];
        assert_eq!(pl.name, "Top Hits");
        assert_eq!(pl.description, "Por Spotify");
        assert_eq!(
            pl.artwork_url.as_deref(),
            Some("https://i.scdn.co/image/cover.jpg")
        );
        assert_eq!(pl.tracks.len(), 1);
        assert_eq!(pl.tracks[0].artist_names(), "Justice, Tame Impala");
        assert_eq!(pl.tracks[0].duration_micros(), 224_877_000);

        let track_html = r#"<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"type":"track","uri":"spotify:track:2iblMMIgSznA464mNov7A8","title":"Don't Stop the Party","artists":[{"name":"Pitbull"},{"name":"TJR"}],"duration":206120,"visualIdentity":{"image":[{"url":"https://i.scdn.co/image/300.jpg","maxWidth":300}]}}}}}}}</script>"#;
        let track_outcome = parse_embed_html(
            track_html,
            SpotifyTargetKind::Track,
            "2iblMMIgSznA464mNov7A8",
        )
        .unwrap();
        assert_eq!(track_outcome.liked_tracks.len(), 1);
        assert_eq!(track_outcome.liked_tracks[0].artist_names(), "Pitbull, TJR");
        assert_eq!(
            track_outcome.liked_tracks[0].artwork_url().as_deref(),
            Some("https://i.scdn.co/image/300.jpg")
        );
    }

    #[test]
    fn parses_spotify_privacy_exports_and_exportify_csv() {
        let playlist_json = br#"{
          "playlists": [
            {
              "name": "Treino",
              "description": "Minhas favoritas",
              "items": [
                {
                  "track": {
                    "trackName": "Numb",
                    "artistName": "Linkin Park",
                    "albumName": "Meteora",
                    "trackUri": "spotify:track:2nLtzopw4rPReszdYBJU6h"
                  }
                }
              ]
            }
          ]
        }"#;
        let pl_out = parse_json_bytes(playlist_json, true).unwrap();
        assert_eq!(pl_out.playlists.len(), 1);
        assert_eq!(pl_out.playlists[0].name, "Treino");
        assert_eq!(pl_out.playlists[0].tracks[0].album_name(), "Meteora");

        let library_json = br#"{
          "tracks": [
            {
              "artist": "Evanescence",
              "album": "Fallen",
              "track": "Going Under",
              "uri": "spotify:track:3UygY7qW2cvG9L5z3S0m8I"
            }
          ]
        }"#;
        let lib_out = parse_json_bytes(library_json, true).unwrap();
        assert_eq!(lib_out.liked_tracks.len(), 1);
        assert_eq!(lib_out.liked_tracks[0].name, "Going Under");

        let history_json = br#"[
          {"endTime":"2026-10-01 12:00","artistName":"Paramore","trackName":"Decode","msPlayed":15000},
          {"endTime":"2026-10-02 12:00","artistName":"Paramore","trackName":"Decode","msPlayed":254000}
        ]"#;
        let hist_out = parse_json_bytes(history_json, true).unwrap();
        assert_eq!(hist_out.playlists.len(), 1);
        assert_eq!(hist_out.playlists[0].name, "Histórico do Spotify");
        assert_eq!(hist_out.playlists[0].tracks.len(), 1);
        assert_eq!(hist_out.playlists[0].tracks[0].duration_micros(), 0);
        assert!(matches!(
            hist_out.playlists[0].tracks[0].id,
            TrackId::Spotify(_)
        ));

        let csv_data = "\u{feff}\"Track URI\",\"Track Name\",\"Artist Name(s)\",\"Album Name\",\"Track Duration (ms)\",\"Album Image URL\"\n\
            \"spotify:track:2nLtzopw4rPReszdYBJU6h\",\"Numb\",\"Linkin Park\",\"Meteora\",\"185586\",\"https://i.scdn.co/image/meteora.jpg\"\n";
        let csv_out = parse_csv_str(csv_data, "Rock_Classics", true).unwrap();
        assert_eq!(csv_out.playlists.len(), 1);
        assert_eq!(csv_out.playlists[0].name, "Rock Classics");
        assert_eq!(csv_out.playlists[0].tracks.len(), 1);
        assert_eq!(
            csv_out.playlists[0].tracks[0].duration_micros(),
            185_586_000
        );
        assert_eq!(
            csv_out.playlists[0].tracks[0].artwork_url().as_deref(),
            Some("https://i.scdn.co/image/meteora.jpg")
        );
    }

    #[test]
    #[ignore = "consulta página pública de embed do Spotify"]
    fn fetches_live_public_spotify_playlist_embed() {
        let outcome = import_url_or_uri(
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            None,
        )
        .unwrap();
        assert_eq!(outcome.playlists.len(), 1);
        assert!(!outcome.playlists[0].tracks.is_empty());
    }
}
