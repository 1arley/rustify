use std::path::PathBuf;

pub(super) fn load_client_id() -> Option<String> {
    if let Ok(value) = std::env::var("SPOTIFY_CLIENT_ID") {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value.to_owned());
        }
    }

    std::fs::read_to_string(config_path()?)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(super) fn save_client_id(client_id: &str) -> Result<String, String> {
    let client_id = client_id.trim();
    if client_id.is_empty() || client_id.len() > 128 || client_id.chars().any(char::is_control) {
        return Err("Informe um Client ID válido do Spotify Developer Dashboard.".to_owned());
    }

    let path = config_path()
        .ok_or_else(|| "Não foi possível localizar a pasta de configuração.".to_owned())?;
    let parent = path
        .parent()
        .ok_or_else(|| "Não foi possível localizar a pasta de configuração.".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Não foi possível criar a configuração: {error}"))?;
    std::fs::write(path, client_id)
        .map_err(|error| format!("Não foi possível salvar o Client ID: {error}"))?;
    Ok(client_id.to_owned())
}

pub(super) fn config_dir() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(root.join("rustify"))
}

fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("client_id"))
}
