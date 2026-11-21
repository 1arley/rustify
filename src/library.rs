use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::catalog::Track;

pub(crate) fn load_liked_tracks() -> Result<Vec<Track>, String> {
    load_from(&storage_path()?)
}

fn load_from(path: &Path) -> Result<Vec<Track>, String> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Não foi possível ler a biblioteca local: {error}")),
    };
    let tracks: Vec<Track> = serde_json::from_slice(&contents)
        .map_err(|error| format!("A biblioteca local não contém dados válidos: {error}"))?;
    Ok(tracks)
}

pub(crate) fn save_liked_tracks(tracks: &[Track]) -> Result<(), String> {
    save_to(&storage_path()?, tracks)
}

fn save_to(path: &Path, tracks: &[Track]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "O diretório da biblioteca local é inválido.".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Não foi possível criar a biblioteca local: {error}"))?;

    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let contents = serde_json::to_vec_pretty(tracks)
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

fn storage_path() -> Result<PathBuf, String> {
    let data_dir = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| "XDG_DATA_HOME e HOME não estão definidos.".to_owned())?;
    Ok(data_dir.join("rea-spotify/liked-tracks.json"))
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
        let liked = load_from(&path).unwrap();
        save_to(&path, &liked).unwrap();
        let restored = load_from(&path).unwrap();
        assert_eq!(restored.len(), 2);
        for (before, after) in liked.iter().zip(restored) {
            assert_eq!(before.id, after.id);
            assert_eq!(before.stream_url(), after.stream_url());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
