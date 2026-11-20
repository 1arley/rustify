use serde::Deserialize;

use super::{api_message, http_agent, search_url, status_error};

pub(super) fn search_tracks(access_token: &str, query: &str) -> Result<Vec<Track>, ApiFailure> {
    let mut response = http_agent()
        .get(search_url(query).as_str())
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|error| ApiFailure {
            message: format!("Não foi possível acessar a API do Spotify: {error}"),
            unauthorized: false,
        })?;

    let status = response.status().as_u16();
    if status >= 400 {
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|value| value.to_str().ok());
        let fallback = status_error(status, retry_after);
        let body = response.body_mut().read_to_string().unwrap_or_default();
        return Err(ApiFailure {
            message: if status == 429 {
                fallback
            } else {
                api_message(&body).unwrap_or(fallback)
            },
            unauthorized: status == 401,
        });
    }

    let response = response
        .body_mut()
        .read_json::<SearchResponse>()
        .map_err(|error| ApiFailure {
            message: format!("Resposta de busca inválida: {error}"),
            unauthorized: false,
        })?;

    Ok(response
        .tracks
        .items
        .into_iter()
        .filter(|track| super::is_valid_track_uri(&track.uri))
        .collect())
}

pub(super) struct ApiFailure {
    pub message: String,
    pub unauthorized: bool,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    tracks: SearchPage,
}

#[derive(Deserialize, Default)]
struct SearchPage {
    #[serde(default)]
    items: Vec<Track>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct Track {
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    artists: Vec<Artist>,
    album: Option<Album>,
}

impl Track {
    pub(crate) fn artist_names(&self) -> String {
        self.artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn album_name(&self) -> &str {
        self.album.as_ref().map_or("", |album| album.name.as_str())
    }
}

#[derive(Clone, Deserialize)]
struct Artist {
    #[serde(default)]
    name: String,
}

#[derive(Clone, Deserialize)]
struct Album {
    #[serde(default)]
    name: String,
}
