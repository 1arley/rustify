mod api;
mod auth;
mod config;
mod import;

use std::time::Duration;

use serde::Deserialize;

pub(crate) use api::{ApiPage, Playlist, Track};
pub(crate) use import::{
    ImportOutcome, import_path, import_url_or_uri, is_spotify_url_or_uri, scan_downloads,
    sync_account,
};

pub(crate) fn client_id() -> Option<String> {
    config::load_client_id()
}

pub(crate) fn save_client_id(client_id: &str) -> Result<String, String> {
    config::save_client_id(client_id)
}

pub(crate) fn is_connected(client_id: &str) -> bool {
    let client_id = client_id.trim();
    !client_id.is_empty() && auth::has_token(client_id)
}

pub(crate) fn connect(client_id: &str) -> Result<(), String> {
    auth::connect(client_id)
}

pub(crate) fn disconnect(client_id: &str) -> Result<(), String> {
    auth::clear_token(client_id.trim())
}

pub(crate) fn is_valid_track_uri(uri: &str) -> bool {
    let Some(id) = uri.strip_prefix("spotify:track:") else {
        return false;
    };
    is_valid_id(id)
}

pub(crate) fn saved_tracks(client_id: &str, offset: u32) -> Result<ApiPage<Track>, String> {
    with_access_token(client_id, |token| api::saved_tracks(token, offset))
}

pub(crate) fn playlists(client_id: &str, offset: u32) -> Result<ApiPage<Playlist>, String> {
    with_access_token(client_id, |token| api::playlists(token, offset))
}

pub(crate) fn playlist_meta(client_id: &str, playlist_id: &str) -> Result<Playlist, String> {
    with_access_token(client_id, |token| api::playlist_meta(token, playlist_id))
}

pub(crate) fn playlist_tracks(
    client_id: &str,
    playlist_id: &str,
    offset: u32,
) -> Result<ApiPage<Track>, String> {
    with_access_token(client_id, |token| {
        api::playlist_tracks(token, playlist_id, offset)
    })
}

fn with_access_token<T>(
    client_id: &str,
    request: impl Fn(&str) -> Result<T, api::ApiFailure>,
) -> Result<T, String> {
    let token = auth::access_token(client_id)?;
    match request(&token) {
        Ok(value) => Ok(value),
        Err(error) if error.unauthorized => {
            let refreshed = match auth::refresh_access_token(client_id) {
                Ok(token) => token,
                Err(error) => {
                    let _ = auth::clear_token(client_id);
                    return Err(error);
                }
            };
            match request(&refreshed) {
                Ok(value) => Ok(value),
                Err(error) => {
                    if error.unauthorized {
                        let _ = auth::clear_token(client_id);
                    }
                    Err(error.message)
                }
            }
        }
        Err(error) => Err(error.message),
    }
}

pub(super) fn is_valid_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn http_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(config)
}

fn status_error(status: u16, retry_after: Option<&str>) -> String {
    match status {
        401 => "Sessão Spotify expirada. Conecte a conta novamente.".to_owned(),
        403 => "O Spotify recusou esta ação para a conta ou aplicativo configurado.".to_owned(),
        429 => match retry_after {
            Some(seconds) => format!("Limite temporário da API. Tente novamente em {seconds} s."),
            None => "Limite temporário da API. Tente novamente em alguns instantes.".to_owned(),
        },
        _ => format!("A API do Spotify respondeu com HTTP {status}."),
    }
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: Option<String>,
}

fn api_message(body: &str) -> Option<String> {
    serde_json::from_str::<ErrorResponse>(body)
        .ok()?
        .error?
        .message
}
