use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::catalog::{Track, TrackId, canonical_song_title, normalize_artist, youtube_id};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct SavedPlaylist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artwork_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(default)]
    pub tracks: Vec<Track>,
}

static LIBRARY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static RESOLVED_CACHE: OnceLock<Mutex<Option<HashMap<String, String>>>> = OnceLock::new();

fn library_lock() -> &'static Mutex<()> {
    LIBRARY_LOCK.get_or_init(|| Mutex::new(()))
}

fn resolved_cache() -> &'static Mutex<Option<HashMap<String, String>>> {
    RESOLVED_CACHE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn load_liked_tracks() -> Result<Vec<Track>, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut tracks = load_json_or_default::<Vec<Track>>(&storage_path("liked-tracks.json")?)?;
    let resolved = load_resolved_map_locked();
    apply_resolved_links(&mut tracks, &resolved);
    Ok(tracks)
}

pub(crate) fn save_liked_tracks(tracks: &[Track]) -> Result<(), String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut updated = tracks.to_vec();
    let resolved = load_resolved_map_locked();
    apply_resolved_links(&mut updated, &resolved);
    save_json_atomic(&storage_path("liked-tracks.json")?, &updated)
}

pub(crate) fn load_playlists() -> Result<Vec<SavedPlaylist>, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut playlists =
        load_json_or_default::<Vec<SavedPlaylist>>(&storage_path("playlists.json")?)?;
    let resolved = load_resolved_map_locked();
    for playlist in &mut playlists {
        apply_resolved_links(&mut playlist.tracks, &resolved);
    }
    Ok(playlists)
}

pub(crate) fn save_playlists(playlists: &[SavedPlaylist]) -> Result<(), String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut updated = playlists.to_vec();
    let resolved = load_resolved_map_locked();
    for playlist in &mut updated {
        apply_resolved_links(&mut playlist.tracks, &resolved);
    }
    save_json_atomic(&storage_path("playlists.json")?, &updated)
}

pub(crate) fn lookup_resolved_youtube_id(spotify_key: &str) -> Option<String> {
    if spotify_key.is_empty() {
        return None;
    }
    let mut cache = resolved_cache().lock().unwrap_or_else(|e| e.into_inner());
    if cache.is_none() {
        let map = storage_path("resolved-tracks.json")
            .ok()
            .and_then(|path| load_json_or_default::<HashMap<String, String>>(&path).ok())
            .unwrap_or_default();
        *cache = Some(map);
    }
    cache
        .as_ref()
        .and_then(|map| map.get(spotify_key))
        .filter(|yt_id| youtube_id(yt_id))
        .cloned()
}

pub(crate) fn record_resolved_youtube_id(spotify_key: &str, yt_id: &str) {
    if spotify_key.is_empty() || !youtube_id(yt_id) {
        return;
    }
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut map = load_resolved_map_locked();
    map.insert(spotify_key.to_owned(), yt_id.to_owned());
    {
        let mut cache = resolved_cache().lock().unwrap_or_else(|e| e.into_inner());
        *cache = Some(map.clone());
    }

    if let Ok(path) = storage_path("resolved-tracks.json") {
        let _ = save_json_atomic(&path, &map);
    }
    if let Ok(path) = storage_path("liked-tracks.json")
        && let Ok(mut tracks) = load_json_or_default::<Vec<Track>>(&path)
        && apply_single_resolved_link(&mut tracks, spotify_key, yt_id)
    {
        let _ = save_json_atomic(&path, &tracks);
    }
    if let Ok(path) = storage_path("playlists.json")
        && let Ok(mut playlists) = load_json_or_default::<Vec<SavedPlaylist>>(&path)
    {
        let mut changed = false;
        for playlist in &mut playlists {
            if apply_single_resolved_link(&mut playlist.tracks, spotify_key, yt_id) {
                changed = true;
            }
        }
        if changed {
            let _ = save_json_atomic(&path, &playlists);
        }
    }
}

pub(crate) fn merge_liked_tracks(existing: &mut Vec<Track>, incoming: Vec<Track>) -> usize {
    let mut added = 0;
    for track in incoming {
        if track.name.trim().is_empty() {
            continue;
        }
        if !existing.iter().any(|cur| tracks_match(cur, &track)) {
            existing.push(track);
            added += 1;
        }
    }
    added
}

pub(crate) fn merge_playlists(
    existing: &mut Vec<SavedPlaylist>,
    incoming: Vec<SavedPlaylist>,
) -> (usize, usize) {
    let mut playlists_affected = 0;
    let mut tracks_added = 0;

    for mut playlist in incoming {
        let mut deduped = Vec::with_capacity(playlist.tracks.len());
        for track in playlist.tracks.drain(..) {
            if track.name.trim().is_empty() {
                continue;
            }
            if !deduped.iter().any(|cur| tracks_match(cur, &track)) {
                deduped.push(track);
            }
        }
        playlist.tracks = deduped;

        let target = existing.iter_mut().find(|cur| {
            cur.id == playlist.id
                || (!cur.name.trim().is_empty()
                    && cur.name.trim().eq_ignore_ascii_case(playlist.name.trim()))
        });

        if let Some(target) = target {
            let mut updated = false;
            if target.description.trim().is_empty() && !playlist.description.trim().is_empty() {
                target.description = playlist.description;
                updated = true;
            }
            if target.artwork_url.is_none() && playlist.artwork_url.is_some() {
                target.artwork_url = playlist.artwork_url;
                updated = true;
            }
            if target.source_url.is_none() && playlist.source_url.is_some() {
                target.source_url = playlist.source_url;
                updated = true;
            }
            let added = merge_liked_tracks(&mut target.tracks, playlist.tracks);
            tracks_added += added;
            if added > 0 || updated {
                playlists_affected += 1;
            }
        } else {
            tracks_added += playlist.tracks.len();
            playlists_affected += 1;
            existing.push(playlist);
        }
    }

    (playlists_affected, tracks_added)
}

fn tracks_match(a: &Track, b: &Track) -> bool {
    if a.same_identity(b) {
        return true;
    }
    let a_title = canonical_song_title(&a.name, a.artist_names());
    let b_title = canonical_song_title(&b.name, b.artist_names());
    let a_artist = normalize_artist(a.artist_names(), &a.name);
    let b_artist = normalize_artist(b.artist_names(), &b.name);
    !a_title.is_empty() && !a_artist.is_empty() && a_title == b_title && a_artist == b_artist
}

fn load_resolved_map_locked() -> HashMap<String, String> {
    let mut cache = resolved_cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = cache.as_ref() {
        return map.clone();
    }
    let map = storage_path("resolved-tracks.json")
        .ok()
        .and_then(|path| load_json_or_default::<HashMap<String, String>>(&path).ok())
        .unwrap_or_default();
    *cache = Some(map.clone());
    map
}

fn apply_resolved_links(tracks: &mut [Track], resolved: &HashMap<String, String>) {
    if resolved.is_empty() {
        return;
    }
    for track in tracks {
        if let TrackId::Spotify(sp_key) = &track.id
            && let Some(yt_id) = resolved.get(sp_key)
            && youtube_id(yt_id)
        {
            track.id = TrackId::Youtube(yt_id.clone());
        }
    }
}

fn apply_single_resolved_link(tracks: &mut [Track], spotify_key: &str, yt_id: &str) -> bool {
    let mut changed = false;
    for track in tracks {
        if let TrackId::Spotify(sp_key) = &track.id
            && sp_key == spotify_key
        {
            track.id = TrackId::Youtube(yt_id.to_owned());
            changed = true;
        }
    }
    changed
}

fn load_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(T::default()),
        Err(error) => return Err(format!("Não foi possível ler a biblioteca local: {error}")),
    };
    serde_json::from_slice(&contents)
        .map_err(|error| format!("A biblioteca local não contém dados válidos: {error}"))
}

fn save_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "O diretório da biblioteca local é inválido.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Não foi possível criar a biblioteca local: {error}"))?;

    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let contents = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Não foi possível serializar a biblioteca local: {error}"))?;
    let write_result = fs::write(&temporary, contents).and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = write_result {
        let _ = fs::remove_file(temporary);
        return Err(format!(
            "Não foi possível salvar a biblioteca local: {error}"
        ));
    }
    Ok(())
}

fn storage_path(file_name: &str) -> Result<PathBuf, String> {
    let data_dir = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| "XDG_DATA_HOME e HOME não estão definidos.".to_owned())?;
    Ok(data_dir.join("rea-spotify").join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reopen_existing_likes_on_disk() {
        let dir = env::temp_dir().join(format!("rea-likes-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("liked-tracks.json");
        fs::write(
            &path,
            r#"[{"id":"abc","title":"Versão A"},{"id":"yt:abcdefghijk","title":"Versão B"}]"#,
        )
        .unwrap();
        let liked = load_json_or_default::<Vec<Track>>(&path).unwrap();
        save_json_atomic(&path, &liked).unwrap();
        let restored = load_json_or_default::<Vec<Track>>(&path).unwrap();
        assert_eq!(restored.len(), 2);
        for (before, after) in liked.iter().zip(restored) {
            assert_eq!(before.id, after.id);
            assert_eq!(before.stream_url(), after.stream_url());
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn persist_and_merge_playlists_and_resolved_links() {
        let dir = env::temp_dir().join(format!("rea-playlists-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("playlists.json");

        let t1 = Track::from_spotify(
            Some("spotify:track:4HCOHRCSQelz2GPX8iTamO"),
            "Flower In A Hurricane".to_owned(),
            "Justice".to_owned(),
            "Album A".to_owned(),
            224_877,
            None,
        );
        let t2 = Track::from_spotify(
            Some("spotify:track:2iblMMIgSznA464mNov7A8"),
            "Don't Stop the Party".to_owned(),
            "Pitbull, TJR".to_owned(),
            "Global Warming".to_owned(),
            206_120,
            None,
        );

        let mut playlists = vec![SavedPlaylist {
            id: "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned(),
            name: "Today's Top Hits".to_owned(),
            description: String::new(),
            artwork_url: None,
            source_url: None,
            tracks: vec![t1.clone()],
        }];

        let incoming = vec![SavedPlaylist {
            id: "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned(),
            name: "Today's Top Hits".to_owned(),
            description: "Por Spotify".to_owned(),
            artwork_url: Some("https://i.scdn.co/image/cover.jpg".to_owned()),
            source_url: Some("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_owned()),
            tracks: vec![t1, t2],
        }];

        let (affected, added) = merge_playlists(&mut playlists, incoming);
        assert_eq!(affected, 1);
        assert_eq!(added, 1);
        assert_eq!(playlists[0].tracks.len(), 2);
        assert_eq!(playlists[0].description, "Por Spotify");

        save_json_atomic(&path, &playlists).unwrap();
        let mut loaded = load_json_or_default::<Vec<SavedPlaylist>>(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].tracks.len(), 2);

        assert!(apply_single_resolved_link(
            &mut loaded[0].tracks,
            "4HCOHRCSQelz2GPX8iTamO",
            "abcdefghijk"
        ));
        assert_eq!(
            loaded[0].tracks[0].id,
            TrackId::Youtube("abcdefghijk".to_owned())
        );

        fs::remove_dir_all(dir).unwrap();
    }
}
