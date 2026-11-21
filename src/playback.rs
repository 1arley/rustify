use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

use libmpv::Mpv;
use rand::Rng;
use url::Url;

#[derive(Clone, Debug)]
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
    pub queue: Vec<PlaybackItem>,
    pub current_index: Option<usize>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            connected: false,
            playing: false,
            title: String::new(),
            artist: String::new(),
            position: 0,
            duration: 0,
            volume: 0.5,
            shuffle: false,
            repeat: "None".to_owned(),
            queue: Vec::new(),
            current_index: None,
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaybackItem {
    pub url: String,
    pub title: String,
    pub artist: String,
    pub duration: i64,
}

#[derive(Clone)]
pub(crate) enum QueueCommand {
    Enqueue(PlaybackItem),
    Remove(usize),
    Move { from: usize, to: usize },
}

enum Request {
    Play(
        Vec<PlaybackItem>,
        usize,
        mpsc::SyncSender<Result<(), String>>,
    ),
    Command(PlaybackCommand, mpsc::SyncSender<Result<(), String>>),
    Queue(QueueCommand, mpsc::SyncSender<Result<(), String>>),
}

struct Player {
    sender: mpsc::Sender<Request>,
    state: Arc<Mutex<PlaybackState>>,
    startup_error: Option<String>,
}

static PLAYER: OnceLock<Player> = OnceLock::new();

pub(crate) fn read_state() -> Result<PlaybackState, String> {
    let player = player();
    player
        .state
        .lock()
        .map(|state| state.clone())
        .map_err(|_| "O estado do player de áudio ficou indisponível.".to_owned())
}

pub(crate) fn run(command: PlaybackCommand) -> Result<(), String> {
    request(|reply| Request::Command(command, reply))
}

pub(crate) fn play_tracks(queue: Vec<PlaybackItem>, selected: usize) -> Result<(), String> {
    if queue.is_empty() || selected >= queue.len() {
        return Err("A fila não contém uma faixa válida.".to_owned());
    }
    for item in &queue {
        validate_audio_url(&item.url)?;
    }
    request(|reply| Request::Play(queue, selected, reply))
}

pub(crate) fn update_queue(command: QueueCommand) -> Result<(), String> {
    if let QueueCommand::Enqueue(item) = &command {
        validate_audio_url(&item.url)?;
    }
    request(|reply| Request::Queue(command, reply))
}

fn validate_audio_url(value: &str) -> Result<(), String> {
    let url = Url::parse(value).map_err(|_| "Endereço de áudio inválido.".to_owned())?;
    if url.scheme() != "https" || url.host_str() != Some("api.audius.co") {
        return Err("O áudio precisa vir do endpoint oficial do Audius.".to_owned());
    }
    Ok(())
}

pub(crate) fn format_time(microseconds: i64) -> String {
    let seconds = microseconds.max(0) / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn request(
    make_request: impl FnOnce(mpsc::SyncSender<Result<(), String>>) -> Request,
) -> Result<(), String> {
    let player = player();
    if let Some(error) = &player.startup_error {
        return Err(error.clone());
    }
    let (reply, response) = mpsc::sync_channel(1);
    player
        .sender
        .send(make_request(reply))
        .map_err(|_| "O processo de áudio foi encerrado.".to_owned())?;
    response
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "O motor de áudio não respondeu a tempo.".to_owned())?
}

fn player() -> &'static Player {
    PLAYER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(Mutex::new(PlaybackState::default()));
        let worker_state = state.clone();
        let startup_error = thread::Builder::new()
            .name("rea-audio".to_owned())
            .spawn(move || run_player(receiver, worker_state))
            .err()
            .map(|error| format!("Não foi possível iniciar o motor de áudio: {error}"));
        Player {
            sender,
            state,
            startup_error,
        }
    })
}

fn run_player(receiver: mpsc::Receiver<Request>, state: Arc<Mutex<PlaybackState>>) {
    let mpv = Mpv::new();
    if let Ok(mpv) = &mpv {
        let _ = mpv.set_property("volume", 50.0_f64);
        if let Ok(mut state) = state.lock() {
            state.connected = true;
        }
    }

    let mut queue = Vec::new();
    let mut current = 0;
    let mut active_index = None;
    let mut shuffle = false;
    let mut repeat = RepeatMode::None;
    let mut history = Vec::new();
    let mut ended = false;
    let mut eof_handled = false;

    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(Request::Play(items, selected, reply)) => {
                queue = items;
                current = selected;
                active_index = Some(selected);
                history.clear();
                ended = false;
                let result = mpv
                    .as_ref()
                    .map_err(|error| error.to_string())
                    .and_then(|mpv| start_current(mpv, &queue, current, &state));
                publish_queue(&state, &queue, active_index);
                if result.is_ok() {
                    eof_handled = true;
                }
                let _ = reply.send(result);
            }
            Ok(Request::Queue(command, reply)) => {
                let result = apply_queue_command(
                    command,
                    &mut queue,
                    &mut current,
                    &mut active_index,
                    &mut history,
                );
                if result.is_ok() {
                    publish_queue(&state, &queue, active_index);
                }
                let _ = reply.send(result);
            }
            Ok(Request::Command(command, reply)) => {
                let result = mpv
                    .as_ref()
                    .map_err(|error| error.to_string())
                    .and_then(|mpv| {
                        run_command(
                            mpv,
                            command,
                            &queue,
                            &mut current,
                            &mut shuffle,
                            &mut repeat,
                            &mut history,
                            &mut ended,
                            &state,
                        )
                    });
                if result.is_ok()
                    && matches!(command, PlaybackCommand::Next | PlaybackCommand::Previous)
                {
                    active_index = Some(current);
                    eof_handled = true;
                }
                publish_queue(&state, &queue, active_index);
                let _ = reply.send(result);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let Ok(mpv) = &mpv else {
            continue;
        };
        update_state(
            mpv,
            &queue,
            current,
            active_index,
            shuffle,
            repeat,
            ended,
            &state,
        );
        if mpv.get_property::<bool>("eof-reached").unwrap_or(false) {
            if !eof_handled {
                eof_handled = true;
                if repeat == RepeatMode::Track {
                    ended = start_current(mpv, &queue, current, &state).is_err();
                } else if let Some(next) = next_index(current, queue.len(), shuffle, repeat) {
                    history.push(current);
                    current = next;
                    active_index = Some(current);
                    ended = start_current(mpv, &queue, current, &state).is_err();
                } else {
                    ended = true;
                    if let Ok(mut state) = state.lock() {
                        state.playing = false;
                    }
                }
            }
        } else {
            eof_handled = false;
        }
    }
}

fn apply_queue_command(
    command: QueueCommand,
    queue: &mut Vec<PlaybackItem>,
    current: &mut usize,
    active_index: &mut Option<usize>,
    history: &mut Vec<usize>,
) -> Result<(), String> {
    match command {
        QueueCommand::Enqueue(item) => queue.push(item),
        QueueCommand::Remove(index) => {
            if index >= queue.len() {
                return Err("A faixa não existe na fila.".to_owned());
            }
            if *active_index == Some(index) {
                return Err("A faixa que está tocando não pode ser removida da fila.".to_owned());
            }
            queue.remove(index);
            history.retain(|played| *played != index);
            for played in history {
                if *played > index {
                    *played -= 1;
                }
            }
            if index < *current {
                *current -= 1;
            }
            if active_index.is_some_and(|active| index < active) {
                *active_index = active_index.map(|active| active - 1);
            }
        }
        QueueCommand::Move { from, to } => {
            let first_upcoming = active_index.map_or(0, |index| index + 1);
            if from >= queue.len() || to >= queue.len() {
                return Err("A posição não existe na fila.".to_owned());
            }
            if from < first_upcoming || to < first_upcoming {
                return Err("Só é possível reordenar as próximas faixas.".to_owned());
            }
            let item = queue.remove(from);
            queue.insert(to, item);
            history.clear();
        }
    }
    if active_index.is_none() {
        *current = 0;
    }
    Ok(())
}

fn publish_queue(
    state: &Arc<Mutex<PlaybackState>>,
    queue: &[PlaybackItem],
    current_index: Option<usize>,
) {
    if let Ok(mut state) = state.lock() {
        if state.queue != queue {
            state.queue = queue.to_vec();
        }
        state.current_index = current_index;
    }
}

#[allow(clippy::too_many_arguments)]
fn run_command(
    mpv: &Mpv,
    command: PlaybackCommand,
    queue: &[PlaybackItem],
    current: &mut usize,
    shuffle: &mut bool,
    repeat: &mut RepeatMode,
    history: &mut Vec<usize>,
    ended: &mut bool,
    state: &Arc<Mutex<PlaybackState>>,
) -> Result<(), String> {
    match command {
        PlaybackCommand::PlayPause => {
            let paused = mpv
                .get_property::<bool>("pause")
                .map_err(|error| error.to_string())?;
            mpv.set_property("pause", !paused)
                .map_err(|error| error.to_string())
        }
        PlaybackCommand::SeekBack => mpv
            .command("seek", &["-10", "relative"])
            .map_err(|error| error.to_string()),
        PlaybackCommand::SeekForward => mpv
            .command("seek", &["10", "relative"])
            .map_err(|error| error.to_string()),
        PlaybackCommand::VolumeDown | PlaybackCommand::VolumeUp => {
            let volume = mpv
                .get_property::<f64>("volume")
                .map_err(|error| error.to_string())?;
            let volume = if matches!(command, PlaybackCommand::VolumeUp) {
                (volume + 5.0).min(100.0)
            } else {
                (volume - 5.0).max(0.0)
            };
            mpv.set_property("volume", volume)
                .map_err(|error| error.to_string())
        }
        PlaybackCommand::Shuffle => {
            *shuffle = !*shuffle;
            if let Ok(mut state) = state.lock() {
                state.shuffle = *shuffle;
            }
            Ok(())
        }
        PlaybackCommand::Repeat => {
            *repeat = match repeat {
                RepeatMode::None => RepeatMode::Playlist,
                RepeatMode::Playlist => RepeatMode::Track,
                RepeatMode::Track => RepeatMode::None,
            };
            if let Ok(mut state) = state.lock() {
                state.repeat = repeat.label().to_owned();
            }
            Ok(())
        }
        PlaybackCommand::Next => move_to_track(
            mpv, queue, current, true, *shuffle, *repeat, history, ended, state,
        ),
        PlaybackCommand::Previous => {
            let position = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
            if position > 3.0 {
                mpv.command("seek", &["0", "absolute"])
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
            if *shuffle && let Some(previous) = history.pop() {
                *current = previous;
                *ended = false;
                return start_current(mpv, queue, *current, state);
            }
            move_to_track(
                mpv, queue, current, false, *shuffle, *repeat, history, ended, state,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn move_to_track(
    mpv: &Mpv,
    queue: &[PlaybackItem],
    current: &mut usize,
    forward: bool,
    shuffle: bool,
    repeat: RepeatMode,
    history: &mut Vec<usize>,
    ended: &mut bool,
    state: &Arc<Mutex<PlaybackState>>,
) -> Result<(), String> {
    let Some(next) = (if forward {
        next_index(*current, queue.len(), shuffle, repeat)
    } else {
        previous_index(*current, queue.len(), repeat)
    }) else {
        *ended = true;
        mpv.command("stop", &[])
            .map_err(|error| error.to_string())?;
        if let Ok(mut state) = state.lock() {
            state.playing = false;
        }
        return Ok(());
    };

    history.push(*current);
    *current = next;
    *ended = false;
    start_current(mpv, queue, next, state)
}

fn next_index(current: usize, length: usize, shuffle: bool, repeat: RepeatMode) -> Option<usize> {
    if length == 0 {
        return None;
    }
    if shuffle && length > 1 {
        let mut next = rand::rng().random_range(0..length);
        if next == current {
            next = (next + 1) % length;
        }
        return Some(next);
    }
    if current + 1 < length {
        Some(current + 1)
    } else if repeat == RepeatMode::Playlist {
        Some(0)
    } else {
        None
    }
}

fn previous_index(current: usize, length: usize, repeat: RepeatMode) -> Option<usize> {
    if length == 0 {
        None
    } else if current > 0 {
        Some(current - 1)
    } else if repeat == RepeatMode::Playlist {
        Some(length - 1)
    } else {
        Some(current)
    }
}

fn start_current(
    mpv: &Mpv,
    queue: &[PlaybackItem],
    current: usize,
    state: &Arc<Mutex<PlaybackState>>,
) -> Result<(), String> {
    let item = queue
        .get(current)
        .ok_or_else(|| "A faixa selecionada não existe na fila.".to_owned())?;
    mpv.command("loadfile", &[&item.url, "replace"])
        .map_err(|error| error.to_string())?;
    if let Ok(mut state) = state.lock() {
        state.title.clone_from(&item.title);
        state.artist.clone_from(&item.artist);
        state.position = 0;
        state.duration = item.duration;
        state.playing = true;
    }
    Ok(())
}

fn update_state(
    mpv: &Mpv,
    queue: &[PlaybackItem],
    current: usize,
    current_index: Option<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    ended: bool,
    state: &Arc<Mutex<PlaybackState>>,
) {
    let position = mpv.get_property::<f64>("time-pos").unwrap_or(0.0).max(0.0);
    let duration = mpv.get_property::<f64>("duration").unwrap_or(0.0).max(0.0);
    let playing = !ended && !mpv.get_property::<bool>("pause").unwrap_or(true);
    if let Ok(mut state) = state.lock() {
        if state.queue != queue {
            state.queue = queue.to_vec();
        }
        state.current_index = current_index;
        state.connected = true;
        state.playing = playing;
        state.position = (position * 1_000_000.0) as i64;
        if duration > 0.0 {
            state.duration = (duration * 1_000_000.0) as i64;
        } else if let Some(item) = queue.get(current) {
            state.duration = item.duration;
        }
        state.volume = mpv.get_property::<f64>("volume").unwrap_or(50.0) / 100.0;
        state.shuffle = shuffle;
        state.repeat = repeat.label().to_owned();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepeatMode {
    None,
    Playlist,
    Track,
}

impl RepeatMode {
    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Playlist => "Playlist",
            Self::Track => "Track",
        }
    }
}
