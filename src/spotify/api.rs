use serde::{Deserialize, de::DeserializeOwned};
use url::Url;

use super::{api_message, http_agent, status_error};

const PAGE_SIZE: &str = "50";

pub(super) fn saved_tracks(access_token: &str, offset: u32) -> Result<ApiPage<Track>, ApiFailure> {
    let mut url = Url::parse("https://api.spotify.com/v1/me/tracks").expect("static API URL");
    add_page_parameters(&mut url, offset);
    let response: SavedTrackResponse = get_json(access_token, url)?;
    Ok(ApiPage::new(
        offset,
        response.items.len(),
        response.total,
        response
            .items
            .into_iter()
            .filter_map(|item| item.track)
            .filter(|track| super::is_valid_track_uri(&track.uri))
            .collect(),
    ))
}

pub(super) fn playlists(access_token: &str, offset: u32) -> Result<ApiPage<Playlist>, ApiFailure> {
    let mut url = Url::parse("https://api.spotify.com/v1/me/playlists").expect("static API URL");
    add_page_parameters(&mut url, offset);
    let response: PlaylistResponse = get_json(access_token, url)?;
    Ok(ApiPage::new(
        offset,
        response.items.len(),
        response.total,
        response
            .items
            .into_iter()
            .filter(|playlist| super::is_valid_id(&playlist.id))
            .collect(),
    ))
}

pub(super) fn playlist_meta(access_token: &str, playlist_id: &str) -> Result<Playlist, ApiFailure> {
    if !super::is_valid_id(playlist_id) {
        return Err(ApiFailure::invalid_response());
    }
    let url = Url::parse(&format!(
        "https://api.spotify.com/v1/playlists/{playlist_id}"
    ))
    .expect("validated playlist ID and static API URL");
    get_json(access_token, url)
}

pub(super) fn playlist_tracks(
    access_token: &str,
    playlist_id: &str,
    offset: u32,
) -> Result<ApiPage<Track>, ApiFailure> {
    if !super::is_valid_id(playlist_id) {
        return Err(ApiFailure::invalid_response());
    }

    let mut url = Url::parse(&format!(
        "https://api.spotify.com/v1/playlists/{playlist_id}/tracks"
    ))
    .expect("validated playlist ID and static API URL");
    add_page_parameters(&mut url, offset);
    let response: PlaylistTrackResponse = get_json(access_token, url)?;
    Ok(ApiPage::new(
        offset,
        response.items.len(),
        response.total,
        response
            .items
            .into_iter()
            .filter_map(|item| item.item)
            .filter(|track| super::is_valid_track_uri(&track.uri))
            .collect(),
    ))
}

fn get_json<T: DeserializeOwned>(access_token: &str, url: Url) -> Result<T, ApiFailure> {
    let mut response = http_agent()
        .get(url.as_str())
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

    response
        .body_mut()
        .read_json::<T>()
        .map_err(|error| ApiFailure {
            message: format!("Resposta da API do Spotify inválida: {error}"),
            unauthorized: false,
        })
}

fn add_page_parameters(url: &mut Url, offset: u32) {
    url.query_pairs_mut()
        .append_pair("limit", PAGE_SIZE)
        .append_pair("offset", &offset.to_string());
}

#[derive(Clone)]
pub(crate) struct ApiPage<T> {
    pub items: Vec<T>,
    pub next_offset: Option<u32>,
}

impl<T> ApiPage<T> {
    fn new(offset: u32, returned: usize, total: u32, items: Vec<T>) -> Self {
        let next_offset = match u32::try_from(returned) {
            Ok(returned) if returned > 0 => {
                offset.checked_add(returned).filter(|next| *next < total)
            }
            _ => None,
        };
        Self { items, next_offset }
    }
}

pub(super) struct ApiFailure {
    pub message: String,
    pub unauthorized: bool,
}

impl ApiFailure {
    fn invalid_response() -> Self {
        Self {
            message: "O Spotify retornou uma playlist inválida.".to_owned(),
            unauthorized: false,
        }
    }
}

#[derive(Clone, Deserialize, Default)]
pub(crate) struct SpotifyImage {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub width: Option<u32>,
}

fn pick_best_image(images: &[SpotifyImage]) -> Option<String> {
    let valid = |img: &&SpotifyImage| {
        let url = img.url.trim();
        url.starts_with("http://") || url.starts_with("https://")
    };
    images
        .iter()
        .filter(valid)
        .find(|img| img.width.is_some_and(|w| (200..=640).contains(&w)))
        .or_else(|| images.iter().find(valid))
        .map(|img| img.url.trim().to_owned())
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
    #[serde(default)]
    pub duration_ms: u64,
}

impl Track {
    pub(crate) fn artist_names(&self) -> String {
        self.artists
            .iter()
            .map(|artist| artist.name.trim())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn album_name(&self) -> &str {
        self.album.as_ref().map_or("", |album| album.name.as_str())
    }

    pub(crate) fn artwork_url(&self) -> Option<String> {
        self.album.as_ref().and_then(Album::artwork_url)
    }

    pub(crate) fn into_catalog_track(self) -> crate::catalog::Track {
        let artist = self.artist_names();
        let album = self.album_name().to_owned();
        let artwork = self.artwork_url();
        crate::catalog::Track::from_spotify(
            Some(&self.uri),
            self.name,
            artist,
            album,
            self.duration_ms,
            artwork,
        )
    }
}

#[derive(Clone, Deserialize)]
pub(crate) struct Playlist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub images: Vec<SpotifyImage>,
}

impl Playlist {
    pub(crate) fn artwork_url(&self) -> Option<String> {
        pick_best_image(&self.images)
    }
}

#[derive(Deserialize)]
struct SavedTrackResponse {
    #[serde(default)]
    items: Vec<SavedTrack>,
    #[serde(default)]
    total: u32,
}

#[derive(Deserialize)]
struct SavedTrack {
    track: Option<Track>,
}

#[derive(Deserialize)]
struct PlaylistResponse {
    #[serde(default)]
    items: Vec<Playlist>,
    #[serde(default)]
    total: u32,
}

#[derive(Deserialize)]
struct PlaylistTrackResponse {
    #[serde(default)]
    items: Vec<PlaylistTrack>,
    #[serde(default)]
    total: u32,
}

#[derive(Deserialize)]
struct PlaylistTrack {
    #[serde(default, alias = "track")]
    item: Option<Track>,
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
    #[serde(default)]
    images: Vec<SpotifyImage>,
}

impl Album {
    fn artwork_url(&self) -> Option<String> {
        pick_best_image(&self.images)
    }
}
