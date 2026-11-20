use std::collections::HashMap;
use std::process::Command as ProcessCommand;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

const PLAYER_NAME: &str = "org.mpris.MediaPlayer2.spotify";
const PLAYER_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone, Debug, Default)]
pub(crate) struct PlaybackState {
    pub connected: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub position: i64,
    pub duration: i64,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: String,
}

#[derive(Clone, Copy)]
pub(crate) enum PlaybackCommand {
    PlayPause,
    Previous,
    Next,
    SeekBack,
    SeekForward,
    VolumeDown,
    VolumeUp,
    Shuffle,
    Repeat,
}

pub(crate) fn read_state() -> Result<PlaybackState, String> {
    let connection = Connection::session().map_err(|error| error.to_string())?;
    if !is_running(&connection)? {
        return Ok(PlaybackState::default());
    }

    let player = player_proxy(&connection)?;
    let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap_or_default();
    let artists: Vec<String> = metadata_value(&metadata, "xesam:artist").unwrap_or_default();

    Ok(PlaybackState {
        connected: true,
        playing: player
            .get_property::<String>("PlaybackStatus")
            .is_ok_and(|status| status == "Playing"),
        title: metadata_value(&metadata, "xesam:title").unwrap_or_default(),
        artist: artists.join(", "),
        position: player.get_property("Position").unwrap_or(0),
        duration: metadata_value(&metadata, "mpris:length").unwrap_or(0),
        volume: player.get_property("Volume").unwrap_or(0.5),
        shuffle: player.get_property("Shuffle").unwrap_or(false),
        repeat: player
            .get_property("LoopStatus")
            .unwrap_or_else(|_| "None".to_owned()),
    })
}

pub(crate) fn run(command: PlaybackCommand) -> Result<(), String> {
    let connection = Connection::session().map_err(|error| error.to_string())?;
    if !is_running(&connection)? {
        return Err("Abra o Spotify Desktop para controlar a reprodução.".to_owned());
    }
    let player = player_proxy(&connection)?;

    match command {
        PlaybackCommand::PlayPause => player
            .call::<_, _, ()>("PlayPause", &())
            .map_err(|error| error.to_string()),
        PlaybackCommand::Previous => player
            .call::<_, _, ()>("Previous", &())
            .map_err(|error| error.to_string()),
        PlaybackCommand::Next => player
            .call::<_, _, ()>("Next", &())
            .map_err(|error| error.to_string()),
        PlaybackCommand::SeekBack => player
            .call::<_, _, ()>("Seek", &(-10_000_000_i64,))
            .map_err(|error| error.to_string()),
        PlaybackCommand::SeekForward => player
            .call::<_, _, ()>("Seek", &(10_000_000_i64,))
            .map_err(|error| error.to_string()),
        PlaybackCommand::VolumeDown => {
            let volume: f64 = player.get_property("Volume").map_err(|e| e.to_string())?;
            player
                .set_property("Volume", (volume - 0.05).max(0.0))
                .map_err(|error| error.to_string())
        }
        PlaybackCommand::VolumeUp => {
            let volume: f64 = player.get_property("Volume").map_err(|e| e.to_string())?;
            if volume >= 1.0 {
                Ok(())
            } else {
                player
                    .set_property("Volume", (volume + 0.05).min(1.0))
                    .map_err(|error| error.to_string())
            }
        }
        PlaybackCommand::Shuffle => {
            let shuffle: bool = player.get_property("Shuffle").map_err(|e| e.to_string())?;
            player
                .set_property("Shuffle", !shuffle)
                .map_err(|error| error.to_string())
        }
        PlaybackCommand::Repeat => {
            let repeat: String = player
                .get_property("LoopStatus")
                .map_err(|e| e.to_string())?;
            let next = match repeat.as_str() {
                "None" => "Playlist",
                "Playlist" => "Track",
                _ => "None",
            };
            player
                .set_property("LoopStatus", next)
                .map_err(|error| error.to_string())
        }
    }
}

pub(crate) fn play_track(uri: &str) -> Result<(), String> {
    if !crate::spotify::is_valid_track_uri(uri) {
        return Err("O Spotify retornou um endereço de faixa inválido.".to_owned());
    }

    if let Ok(connection) = Connection::session()
        && is_running(&connection).unwrap_or(false)
        && let Ok(root) = Proxy::new(
            &connection,
            PLAYER_NAME,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2",
        )
        && root.call::<_, _, ()>("OpenUri", &(uri,)).is_ok()
    {
        return Ok(());
    }

    ProcessCommand::new("spotify")
        .arg(format!("--uri={uri}"))
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Não foi possível abrir a faixa no Spotify Desktop: {error}"))
}

pub(crate) fn format_time(microseconds: i64) -> String {
    let seconds = microseconds.max(0) / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn is_running(connection: &Connection) -> Result<bool, String> {
    let bus = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(|error| error.to_string())?;
    bus.call("NameHasOwner", &(PLAYER_NAME,))
        .map_err(|error| error.to_string())
}

fn player_proxy(connection: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(connection, PLAYER_NAME, PLAYER_PATH, PLAYER_INTERFACE)
        .map_err(|error| error.to_string())
}

fn metadata_value<T>(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    metadata.get(key)?.clone().try_into().ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use zbus::zvariant::{OwnedValue, Value};

    use super::format_time;
    use super::metadata_value;

    #[test]
    fn formats_elapsed_and_negative_positions() {
        assert_eq!(format_time(61_500_000), "1:01");
        assert_eq!(format_time(-1), "0:00");
    }

    #[test]
    fn reads_title_from_mpris_metadata() {
        let title = OwnedValue::try_from(Value::from("Faixa real")).unwrap();
        let metadata = HashMap::from([("xesam:title".to_owned(), title)]);
        assert_eq!(
            metadata_value::<String>(&metadata, "xesam:title").as_deref(),
            Some("Faixa real")
        );
    }
}
