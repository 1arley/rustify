mod api;
mod auth;
mod config;

use std::time::Duration;

use serde::Deserialize;
use url::Url;

pub(crate) use api::Track;

pub(crate) fn client_id() -> Option<String> {
    config::load_client_id()
}

pub(crate) fn save_client_id(client_id: &str) -> Result<String, String> {
    config::save_client_id(client_id)
}

pub(crate) fn connect(client_id: &str) -> Result<(), String> {
    auth::connect(client_id)
}

pub(crate) fn search_tracks(client_id: &str, query: &str) -> Result<Vec<Track>, String> {
    let access_token = auth::access_token(client_id)?;
    match api::search_tracks(&access_token, query) {
        Ok(tracks) => Ok(tracks),
        Err(error) if error.unauthorized => {
            let refreshed = match auth::refresh_access_token(client_id) {
                Ok(token) => token,
                Err(error) => {
                    let _ = auth::clear_token(client_id);
                    return Err(error);
                }
            };
            match api::search_tracks(&refreshed, query) {
                Ok(tracks) => Ok(tracks),
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

pub(crate) fn is_valid_track_uri(uri: &str) -> bool {
    let Some(id) = uri.strip_prefix("spotify:track:") else {
        return false;
    };
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

fn search_url(query: &str) -> Url {
    let mut url = Url::parse("https://api.spotify.com/v1/search").expect("static API URL");
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("type", "track")
        .append_pair("limit", "10");
    url
}
