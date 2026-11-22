use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::catalog::{Track, TrackId, canonical_song_title, normalize_artist, youtube_id};
use crate::playback::PlaybackSession;

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

#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct SpotifySnapshot {
    #[serde(default)]
    pub liked_tracks: Vec<Track>,
    #[serde(default)]
    pub playlists: Vec<SavedPlaylist>,
    #[serde(default)]
    pub resolved_tracks: HashMap<String, String>,
}

static LIBRARY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static RESOLVED_CACHE: OnceLock<Mutex<Option<HashMap<String, String>>>> = OnceLock::new();
static SPOTIFY_RESOLVED_CACHE: OnceLock<Mutex<Option<HashMap<String, String>>>> = OnceLock::new();

fn library_lock() -> &'static Mutex<()> {
    LIBRARY_LOCK.get_or_init(|| Mutex::new(()))
}

fn resolved_cache() -> &'static Mutex<Option<HashMap<String, String>>> {
    RESOLVED_CACHE.get_or_init(|| Mutex::new(None))
}

fn spotify_resolved_cache() -> &'static Mutex<Option<HashMap<String, String>>> {
    SPOTIFY_RESOLVED_CACHE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn load_liked_tracks() -> Result<Vec<Track>, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut tracks = load_local_liked_tracks_locked()?;
    let mut snapshot = load_spotify_snapshot_locked()?;
    apply_resolved_links(&mut snapshot.liked_tracks, &snapshot.resolved_tracks);
    merge_liked_tracks(&mut tracks, snapshot.liked_tracks);
    Ok(tracks)
}

pub(crate) fn load_local_liked_tracks() -> Result<Vec<Track>, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    load_local_liked_tracks_locked()
}

fn load_local_liked_tracks_locked() -> Result<Vec<Track>, String> {
    let mut tracks = load_json_or_default::<Vec<Track>>(&storage_path("liked-tracks.json")?)?;
    apply_resolved_links(&mut tracks, &load_resolved_map_locked());
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
    let mut playlists = load_local_playlists_locked()?;
    let mut account = load_spotify_snapshot_locked()?;
    for playlist in &mut account.playlists {
        apply_resolved_links(&mut playlist.tracks, &account.resolved_tracks);
    }
    merge_playlists(&mut playlists, account.playlists);
    Ok(playlists)
}

pub(crate) fn load_local_playlists() -> Result<Vec<SavedPlaylist>, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    load_local_playlists_locked()
}

fn load_local_playlists_locked() -> Result<Vec<SavedPlaylist>, String> {
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

pub(crate) fn create_local_playlist(name: &str) -> Result<SavedPlaylist, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    create_local_playlist_at(&storage_path("playlists.json")?, name, rand::random())
}

fn create_local_playlist_at(path: &Path, name: &str, id: u64) -> Result<SavedPlaylist, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Informe um nome para a playlist.".to_owned());
    }
    let mut playlists = load_json_or_default::<Vec<SavedPlaylist>>(path)?;
    if playlists
        .iter()
        .any(|playlist| playlist.name.trim().eq_ignore_ascii_case(name))
    {
        return Err("Já existe uma playlist com esse nome.".to_owned());
    }
    let playlist = SavedPlaylist {
        id: format!("local:playlist:{id:016x}"),
        name: name.to_owned(),
        description: String::new(),
        artwork_url: None,
        source_url: None,
        tracks: Vec::new(),
    };
    playlists.push(playlist.clone());
    save_json_atomic(path, &playlists)?;
    Ok(playlist)
}

pub(crate) fn add_track_to_local_playlist(playlist_id: &str, track: Track) -> Result<bool, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    add_track_to_local_playlist_at(&storage_path("playlists.json")?, playlist_id, track)
}

fn add_track_to_local_playlist_at(
    path: &Path,
    playlist_id: &str,
    track: Track,
) -> Result<bool, String> {
    let mut playlists = load_json_or_default::<Vec<SavedPlaylist>>(path)?;
    let Some(playlist) = playlists
        .iter_mut()
        .find(|playlist| playlist.id == playlist_id)
    else {
        return Err("A playlist local não foi encontrada.".to_owned());
    };
    if playlist
        .tracks
        .iter()
        .any(|saved| tracks_match(saved, &track))
    {
        return Ok(false);
    }
    playlist.tracks.push(track);
    save_json_atomic(path, &playlists)?;
    Ok(true)
}

pub(crate) fn load_playback_session() -> Result<Option<PlaybackSession>, String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    load_playback_session_at(&storage_path("playback-session.json")?)
}

pub(crate) fn save_playback_session(session: &PlaybackSession) -> Result<(), String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    save_playback_session_at(&storage_path("playback-session.json")?, session)
}

fn load_playback_session_at(path: &Path) -> Result<Option<PlaybackSession>, String> {
    let session = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<PlaybackSession>(&bytes)
            .map_err(|error| format!("Não foi possível ler a sessão do player: {error}"))?,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Não foi possível ler a sessão do player: {error}")),
    };
    if session.position < 0 {
        return Err("A sessão salva do player é inválida.".to_owned());
    }
    Ok(Some(session))
}

fn save_playback_session_at(path: &Path, session: &PlaybackSession) -> Result<(), String> {
    if session.position < 0 {
        return Err("A posição salva do player é inválida.".to_owned());
    }
    save_json_atomic(path, session)
}

pub(crate) fn lookup_resolved_youtube_id(spotify_key: &str) -> Option<String> {
    if spotify_key.is_empty() {
        return None;
    }
    let local = {
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
    };
    local.or_else(|| {
        let mut cache = spotify_resolved_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if cache.is_none() {
            let map = storage_path("spotify-account.json")
                .ok()
                .and_then(|path| load_spotify_snapshot_at(&path).ok())
                .map(|snapshot| snapshot.resolved_tracks)
                .unwrap_or_default();
            *cache = Some(map);
        }
        cache
            .as_ref()
            .and_then(|map| map.get(spotify_key))
            .filter(|yt_id| youtube_id(yt_id))
            .cloned()
    })
}

pub(crate) fn replace_spotify_snapshot(
    snapshot: Result<SpotifySnapshot, String>,
) -> Result<(), String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let snapshot = replace_spotify_snapshot_at(&storage_path("spotify-account.json")?, snapshot)?;
    *spotify_resolved_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(snapshot.resolved_tracks);
    Ok(())
}

pub(crate) fn clear_spotify_snapshot() -> Result<(), String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_spotify_snapshot_at(&storage_path("spotify-account.json")?)?;
    *spotify_resolved_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(HashMap::new());
    Ok(())
}

pub(crate) fn remove_legacy_spotify_data() -> Result<(usize, usize), String> {
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let removed = remove_legacy_spotify_data_at(
        &storage_path("liked-tracks.json")?,
        &storage_path("playlists.json")?,
    )?;
    let resolved = storage_path("resolved-tracks.json")
        .ok()
        .and_then(|path| load_json_or_default::<HashMap<String, String>>(&path).ok())
        .unwrap_or_default();
    *resolved_cache().lock().unwrap_or_else(|e| e.into_inner()) = Some(resolved);
    Ok(removed)
}

pub(crate) fn record_resolved_youtube_id(spotify_key: &str, yt_id: &str) {
    if spotify_key.is_empty() || !youtube_id(yt_id) {
        return;
    }
    let _guard = library_lock().lock().unwrap_or_else(|e| e.into_inner());
    let snapshot_path = storage_path("spotify-account.json").ok();
    let mut snapshot = snapshot_path
        .as_deref()
        .map(load_spotify_snapshot_at)
        .and_then(Result::ok);
    let is_account_track = snapshot.as_ref().is_some_and(|snapshot| {
        snapshot
            .liked_tracks
            .iter()
            .chain(
                snapshot
                    .playlists
                    .iter()
                    .flat_map(|playlist| &playlist.tracks),
            )
            .any(|track| matches!(&track.id, TrackId::Spotify(id) if id == spotify_key))
    });

    if is_account_track && let Some(account_snapshot) = snapshot.as_mut() {
        account_snapshot
            .resolved_tracks
            .insert(spotify_key.to_owned(), yt_id.to_owned());
        if let Some(path) = snapshot_path.as_deref()
            && save_spotify_snapshot_at(path, account_snapshot).is_ok()
        {
            *spotify_resolved_cache()
                .lock()
                .unwrap_or_else(|e| e.into_inner()) =
                Some(account_snapshot.resolved_tracks.clone());
        }
    }

    let local_tracks = storage_path("liked-tracks.json")
        .ok()
        .and_then(|path| load_json_or_default::<Vec<Track>>(&path).ok())
        .unwrap_or_default();
    let local_playlists = storage_path("playlists.json")
        .ok()
        .and_then(|path| load_json_or_default::<Vec<SavedPlaylist>>(&path).ok())
        .unwrap_or_default();
    let is_local_track = local_tracks
        .iter()
        .chain(local_playlists.iter().flat_map(|playlist| &playlist.tracks))
        .any(|track| matches!(&track.id, TrackId::Spotify(id) if id == spotify_key));

    let update_local = if snapshot.is_some() {
        !is_account_track || is_local_track
    } else {
        is_local_track
    };
    if update_local {
        let mut map = load_resolved_map_locked();
        map.insert(spotify_key.to_owned(), yt_id.to_owned());
        {
            let mut cache = resolved_cache().lock().unwrap_or_else(|e| e.into_inner());
            *cache = Some(map.clone());
        }
        if let Ok(path) = storage_path("resolved-tracks.json") {
            let _ = save_json_atomic(&path, &map);
        }
    }

    if !is_account_track || is_local_track {
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
}

fn load_spotify_snapshot_locked() -> Result<SpotifySnapshot, String> {
    load_spotify_snapshot_at(&storage_path("spotify-account.json")?)
}

fn load_spotify_snapshot_at(path: &Path) -> Result<SpotifySnapshot, String> {
    load_json_or_default(path)
}

fn save_spotify_snapshot_at(path: &Path, snapshot: &SpotifySnapshot) -> Result<(), String> {
    save_json_atomic(path, snapshot)
}

fn replace_spotify_snapshot_at(
    path: &Path,
    snapshot: Result<SpotifySnapshot, String>,
) -> Result<SpotifySnapshot, String> {
    let mut snapshot = snapshot?;
    if let Ok(previous) = load_spotify_snapshot_at(path) {
        let current_tracks = snapshot
            .liked_tracks
            .iter()
            .chain(
                snapshot
                    .playlists
                    .iter()
                    .flat_map(|playlist| &playlist.tracks),
            )
            .filter_map(spotify_track_key)
            .collect::<HashSet<_>>();
        for (key, youtube_id) in previous.resolved_tracks {
            if current_tracks.contains(&key) {
                snapshot.resolved_tracks.entry(key).or_insert(youtube_id);
            }
        }
    }
    save_spotify_snapshot_at(path, &snapshot)?;
    Ok(snapshot)
}

fn clear_spotify_snapshot_at(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Não foi possível remover os dados sincronizados do Spotify: {error}"
        )),
    }
}

fn remove_legacy_spotify_data_at(
    liked_path: &Path,
    playlists_path: &Path,
) -> Result<(usize, usize), String> {
    let mut liked = load_json_or_default::<Vec<Track>>(liked_path)?;
    let previous_liked = liked.len();
    let mut spotify_keys = liked
        .iter()
        .filter_map(spotify_track_key)
        .collect::<HashSet<_>>();
    liked.retain(|track| !is_spotify_linked_track(track));

    let mut playlists = load_json_or_default::<Vec<SavedPlaylist>>(playlists_path)?;
    let previous_playlists = playlists.len();
    let previous_playlist_tracks: usize =
        playlists.iter().map(|playlist| playlist.tracks.len()).sum();
    for track in playlists
        .iter()
        .flat_map(|playlist| playlist.tracks.iter())
        .filter(|track| is_spotify_linked_track(track))
    {
        if let Some(key) = spotify_track_key(track) {
            spotify_keys.insert(key);
        }
    }
    playlists.retain_mut(|playlist| {
        if is_spotify_linked_playlist(playlist) {
            return false;
        }
        playlist
            .tracks
            .retain(|track| !is_spotify_linked_track(track));
        true
    });
    let remaining_playlist_tracks: usize =
        playlists.iter().map(|playlist| playlist.tracks.len()).sum();
    let removed_tracks =
        previous_liked - liked.len() + previous_playlist_tracks - remaining_playlist_tracks;
    let removed_playlists = previous_playlists - playlists.len();
    let resolved_path = liked_path.with_file_name("resolved-tracks.json");
    let mut resolved = load_json_or_default::<HashMap<String, String>>(&resolved_path)?;
    let previous_resolutions = resolved.len();
    resolved.retain(|key, _| !spotify_keys.contains(key));

    if removed_tracks > 0 || removed_playlists > 0 {
        save_json_atomic(liked_path, &liked)?;
        save_json_atomic(playlists_path, &playlists)?;
    }
    if resolved.len() < previous_resolutions {
        save_json_atomic(&resolved_path, &resolved)?;
    }
    Ok((removed_tracks, removed_playlists))
}

fn is_spotify_linked_track(track: &Track) -> bool {
    spotify_track_key(track).is_some()
}

fn spotify_track_key(track: &Track) -> Option<String> {
    if let TrackId::Spotify(key) = &track.id {
        return Some(key.clone());
    }
    let permalink = track.permalink();
    let id = permalink
        .strip_prefix("https://open.spotify.com/track/")?
        .split(['/', '?', '#'])
        .next()?;
    crate::catalog::is_spotify_base62_id(id).then(|| id.to_owned())
}

fn is_spotify_linked_playlist(playlist: &SavedPlaylist) -> bool {
    playlist.id.starts_with("spotify:playlist:")
        || playlist.id.starts_with("spotify:album:")
        || playlist.source_url.as_deref().is_some_and(|url| {
            url.starts_with("https://open.spotify.com/playlist/")
                || url.starts_with("https://open.spotify.com/album/")
        })
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
                || (!is_source_playlist(&cur.id)
                    && !is_source_playlist(&playlist.id)
                    && !cur.name.trim().is_empty()
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

fn is_source_playlist(id: &str) -> bool {
    id.starts_with("spotify:") || id.starts_with("youtube:") || id.starts_with("local:")
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
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| "Não foi possível localizar a pasta de dados do usuário.".to_owned())?;
    Ok(data_dir.join("rustify").join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

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

    #[test]
    fn spotify_snapshot_replaces_only_after_successful_sync() {
        let dir = env::temp_dir().join(format!("rea-spotify-snapshot-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("spotify-account.json");
        let first = Track::from_spotify(
            Some("spotify:track:4HCOHRCSQelz2GPX8iTamO"),
            "First Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );
        let second = Track::from_spotify(
            Some("spotify:track:2iblMMIgSznA464mNov7A8"),
            "Second Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );

        replace_spotify_snapshot_at(
            &path,
            Ok(SpotifySnapshot {
                liked_tracks: vec![first.clone()],
                ..SpotifySnapshot::default()
            }),
        )
        .unwrap();
        assert!(replace_spotify_snapshot_at(&path, Err("sync failed".to_owned())).is_err());
        assert_eq!(
            load_spotify_snapshot_at(&path).unwrap().liked_tracks[0].name,
            "First Track"
        );

        let blocked_temp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::create_dir(&blocked_temp_path).unwrap();
        assert!(
            replace_spotify_snapshot_at(
                &path,
                Ok(SpotifySnapshot {
                    liked_tracks: vec![second.clone()],
                    ..SpotifySnapshot::default()
                })
            )
            .is_err()
        );
        assert_eq!(
            load_spotify_snapshot_at(&path).unwrap().liked_tracks[0].name,
            "First Track"
        );
        fs::remove_dir(&blocked_temp_path).unwrap();

        replace_spotify_snapshot_at(
            &path,
            Ok(SpotifySnapshot {
                liked_tracks: vec![second],
                ..SpotifySnapshot::default()
            }),
        )
        .unwrap();
        let replaced = load_spotify_snapshot_at(&path).unwrap();
        assert_eq!(replaced.liked_tracks.len(), 1);
        assert_eq!(replaced.liked_tracks[0].name, "Second Track");
        assert!(matches!(replaced.liked_tracks[0].id, TrackId::Spotify(_)));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn spotify_snapshot_keeps_resolutions_for_tracks_still_in_account() {
        let dir = env::temp_dir().join(format!("rea-account-resolved-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("spotify-account.json");
        let kept = Track::from_spotify(
            Some("spotify:track:4HCOHRCSQelz2GPX8iTamO"),
            "Kept Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );
        let removed = Track::from_spotify(
            Some("spotify:track:2iblMMIgSznA464mNov7A8"),
            "Removed Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );
        let resolutions = HashMap::from([
            (
                "4HCOHRCSQelz2GPX8iTamO".to_owned(),
                "abcdefghijk".to_owned(),
            ),
            (
                "2iblMMIgSznA464mNov7A8".to_owned(),
                "lmnopqrstuv".to_owned(),
            ),
        ]);
        save_spotify_snapshot_at(
            &path,
            &SpotifySnapshot {
                liked_tracks: vec![kept.clone(), removed],
                resolved_tracks: resolutions,
                ..SpotifySnapshot::default()
            },
        )
        .unwrap();

        let replaced = replace_spotify_snapshot_at(
            &path,
            Ok(SpotifySnapshot {
                liked_tracks: vec![kept],
                ..SpotifySnapshot::default()
            }),
        )
        .unwrap();

        assert_eq!(replaced.resolved_tracks.len(), 1);
        assert_eq!(
            replaced
                .resolved_tracks
                .get("4HCOHRCSQelz2GPX8iTamO")
                .map(String::as_str),
            Some("abcdefghijk")
        );
        assert!(
            !replaced
                .resolved_tracks
                .contains_key("2iblMMIgSznA464mNov7A8")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disconnect_removes_snapshot_but_preserves_local_library() {
        let dir = env::temp_dir().join(format!("rea-disconnect-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let liked_path = dir.join("liked-tracks.json");
        let playlists_path = dir.join("playlists.json");
        let snapshot_path = dir.join("spotify-account.json");
        let mut local_track = Track::from_spotify(
            None,
            "Local Track".to_owned(),
            "Local Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );
        local_track.id = TrackId::Audius("localtrack".to_owned());
        let local_playlist = SavedPlaylist {
            id: "local-playlist".to_owned(),
            name: "Local Playlist".to_owned(),
            description: String::new(),
            artwork_url: None,
            source_url: None,
            tracks: vec![local_track.clone()],
        };
        save_json_atomic(&liked_path, &[local_track.clone()]).unwrap();
        save_json_atomic(&playlists_path, &[local_playlist.clone()]).unwrap();
        save_spotify_snapshot_at(
            &snapshot_path,
            &SpotifySnapshot {
                liked_tracks: vec![Track::from_spotify(
                    Some("spotify:track:4HCOHRCSQelz2GPX8iTamO"),
                    "Account Track".to_owned(),
                    "Account Artist".to_owned(),
                    String::new(),
                    100_000,
                    None,
                )],
                resolved_tracks: HashMap::from([(
                    "4HCOHRCSQelz2GPX8iTamO".to_owned(),
                    "abcdefghijk".to_owned(),
                )]),
                ..SpotifySnapshot::default()
            },
        )
        .unwrap();

        clear_spotify_snapshot_at(&snapshot_path).unwrap();
        assert!(
            load_spotify_snapshot_at(&snapshot_path)
                .unwrap()
                .liked_tracks
                .is_empty()
        );
        assert_eq!(
            load_json_or_default::<Vec<Track>>(&liked_path).unwrap()[0].name,
            "Local Track"
        );
        assert_eq!(
            load_json_or_default::<Vec<SavedPlaylist>>(&playlists_path).unwrap()[0].id,
            "local-playlist"
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_cleanup_removes_only_spotify_linked_items() {
        let dir = env::temp_dir().join(format!("rea-legacy-cleanup-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let liked_path = dir.join("liked-tracks.json");
        let playlists_path = dir.join("playlists.json");
        let spotify_track = Track::from_spotify(
            Some("spotify:track:4HCOHRCSQelz2GPX8iTamO"),
            "Spotify Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );
        let mut resolved_legacy_track = spotify_track.clone();
        resolved_legacy_track.id = TrackId::Youtube("abcdefghijk".to_owned());
        let mut manual_track = Track::from_spotify(
            None,
            "Manual Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );
        manual_track.id = TrackId::Audius("manualtrack".to_owned());
        let mut ambiguous_track = Track::from_spotify(
            None,
            "Ambiguous Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );
        ambiguous_track.id = TrackId::Youtube("ambig000000".to_owned());
        let spotify_playlist = SavedPlaylist {
            id: "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".to_owned(),
            name: "Old Account Playlist".to_owned(),
            description: String::new(),
            artwork_url: None,
            source_url: None,
            tracks: vec![spotify_track.clone()],
        };
        let manual_playlist = SavedPlaylist {
            id: "manual-playlist".to_owned(),
            name: "Manual Playlist".to_owned(),
            description: String::new(),
            artwork_url: None,
            source_url: None,
            tracks: vec![spotify_track, ambiguous_track.clone()],
        };
        save_json_atomic(
            &liked_path,
            &[
                resolved_legacy_track,
                manual_track.clone(),
                ambiguous_track.clone(),
            ],
        )
        .unwrap();
        save_json_atomic(&playlists_path, &[spotify_playlist, manual_playlist]).unwrap();
        let resolved_path = dir.join("resolved-tracks.json");
        save_json_atomic(
            &resolved_path,
            &HashMap::from([
                (
                    "4HCOHRCSQelz2GPX8iTamO".to_owned(),
                    "abcdefghijk".to_owned(),
                ),
                ("unidentified-key".to_owned(), "lmnopqrstuv".to_owned()),
            ]),
        )
        .unwrap();

        let (removed_tracks, removed_playlists) =
            remove_legacy_spotify_data_at(&liked_path, &playlists_path).unwrap();
        assert_eq!((removed_tracks, removed_playlists), (3, 1));
        let liked = load_json_or_default::<Vec<Track>>(&liked_path).unwrap();
        let playlists = load_json_or_default::<Vec<SavedPlaylist>>(&playlists_path).unwrap();
        assert_eq!(liked.len(), 2);
        assert_eq!(liked[0].name, "Manual Track");
        assert_eq!(liked[1].name, "Ambiguous Track");
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].tracks.len(), 1);
        assert_eq!(playlists[0].tracks[0].name, "Ambiguous Track");
        let resolutions = load_json_or_default::<HashMap<String, String>>(&resolved_path).unwrap();
        assert!(!resolutions.contains_key("4HCOHRCSQelz2GPX8iTamO"));
        assert_eq!(
            resolutions.get("unidentified-key").map(String::as_str),
            Some("lmnopqrstuv")
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn local_playlists_can_be_created_and_tracks_are_not_duplicated() {
        let dir = env::temp_dir().join(format!("rea-local-playlists-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("playlists.json");
        let playlist = create_local_playlist_at(&path, "Road trip", 42).unwrap();
        let track = Track::from_spotify(
            Some("spotify:track:4HCOHRCSQelz2GPX8iTamO"),
            "First Track".to_owned(),
            "Artist".to_owned(),
            String::new(),
            100_000,
            None,
        );

        assert!(add_track_to_local_playlist_at(&path, &playlist.id, track.clone()).unwrap());
        assert!(!add_track_to_local_playlist_at(&path, &playlist.id, track).unwrap());
        let saved = load_json_or_default::<Vec<SavedPlaylist>>(&path).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "Road trip");
        assert_eq!(saved[0].tracks.len(), 1);
        assert!(create_local_playlist_at(&path, " ROAD TRIP ", 43).is_err());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn same_name_local_and_imported_playlists_do_not_merge() {
        let mut existing = vec![SavedPlaylist {
            id: "local:playlist:1".to_owned(),
            name: "Mix".to_owned(),
            description: String::new(),
            artwork_url: None,
            source_url: None,
            tracks: Vec::new(),
        }];
        let (affected, _) = merge_playlists(
            &mut existing,
            vec![SavedPlaylist {
                id: "youtube:playlist:PLabc_123".to_owned(),
                name: "Mix".to_owned(),
                description: String::new(),
                artwork_url: None,
                source_url: None,
                tracks: Vec::new(),
            }],
        );
        assert_eq!(affected, 1);
        assert_eq!(existing.len(), 2);
    }

    #[test]
    fn playback_session_roundtrips_current_track_and_position() {
        let dir = env::temp_dir().join(format!("rea-playback-session-{}", rand::random::<u64>()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("playback-session.json");
        let session = PlaybackSession {
            item: crate::playback::PlaybackItem {
                track_id: TrackId::Youtube("abcdefghijk".to_owned()),
                title: "Current Track".to_owned(),
                artist: "Artist".to_owned(),
                artwork_url: None,
                duration: 180_000_000,
            },
            position: 44_000_000,
        };

        save_playback_session_at(&path, &session).unwrap();
        assert_eq!(load_playback_session_at(&path).unwrap(), Some(session));

        fs::remove_dir_all(dir).unwrap();
    }
}
