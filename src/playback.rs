use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

use crate::catalog::TrackId;
use crate::queue::{Entry, Queue};
use libmpv::Mpv;
use libmpv::events::Event;

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
    pub queue: Vec<Entry>,
    pub loading: bool,
    pub retry: bool,
    pub notice: Option<String>,
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
            loading: false,
            retry: false,
            notice: None,
            current_index: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum PlaybackCommand {
    Select(u64),
    PlayPause,
    Previous,
    Next,
    SeekPercent(f32),
    Volume(f32),
    SeekBack,
    SeekForward,
    VolumeDown,
    VolumeUp,
    Shuffle,
    Repeat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaybackItem {
    pub track_id: TrackId,
    pub title: String,
    pub artist: String,
    pub duration: i64,
}

#[derive(Clone)]
pub(crate) enum QueueCommand {
    Enqueue(PlaybackItem),
    Remove(u64),
    Move { from: u64, to: u64 },
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
    request(|reply| Request::Play(queue, selected, reply))
}

pub(crate) fn update_queue(command: QueueCommand) -> Result<(), String> {
    request(|reply| Request::Queue(command, reply))
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

fn engine() -> Result<Mpv, String> {
    Mpv::with_initializer(|init| {
        init.set_option("vid", "no")?;
        init.set_option(
            "ytdl-format",
            "ba[protocol=m3u8_native]/b[protocol=m3u8_native]/ba/b",
        )?;
        init.set_option("ytdl", true)
    })
    .map_err(|e| e.to_string())
}
fn load(mpv: &Mpv, url: &str) -> Result<(), String> {
    mpv.command("loadfile", &[url, "replace"])
        .and_then(|_| mpv.set_property("pause", false))
        .map_err(|e| e.to_string())
}

#[derive(PartialEq, Eq)]
enum Phase {
    Idle,
    Loading,
    Started,
    Ended,
    Failed,
}

struct Worker {
    queue: Queue,
    engine: Result<Mpv, String>,
    phase: Phase,
    automatic: bool,
    notice: Option<String>,
    volume: f64,
}
impl Worker {
    fn start(&mut self, automatic: bool) {
        self.automatic = automatic;
        if !automatic {
            self.notice = None;
        }
        loop {
            let Some(index) = self.queue.index() else {
                return;
            };
            let item = &self.queue.entries[index].item;
            // Each selection owns its event queue, including when a URL is selected twice.
            if self.phase != Phase::Idle || self.engine.is_err() {
                self.engine = engine();
            }
            self.phase = Phase::Loading;
            let result = self.engine.as_ref().map_err(Clone::clone).and_then(|mpv| {
                mpv.set_property("volume", self.volume)
                    .map_err(|e| e.to_string())?;
                load(mpv, &item.track_id.stream_url())
            });
            match result {
                Ok(()) => return,
                Err(e) => {
                    if !self.fail(e) {
                        return;
                    }
                }
            }
        }
    }
    fn fail(&mut self, error: String) -> bool {
        let title = &self.queue.entries[self.queue.index().expect("selected occurrence")]
            .item
            .title;
        self.notice = Some(format!("Falha ao tocar {title}: {error}"));
        self.phase = Phase::Failed;
        if self.automatic
            && let Some(next) = self.queue.failed()
        {
            self.queue.selected = Some(next);
            return true;
        }
        false
    }
    fn advance(&mut self, eof: bool) -> Result<(), String> {
        self.queue.completed();
        if let Some(next) = self.queue.next(eof) {
            self.queue.selected = Some(next);
            self.start(eof);
        } else {
            self.phase = Phase::Ended;
            if !eof {
                self.mpv()?
                    .command("stop", &[])
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    fn mpv(&self) -> Result<&Mpv, String> {
        self.engine.as_ref().map_err(Clone::clone)
    }
    fn poll(&mut self) {
        loop {
            let Ok(mpv) = self.mpv() else { return };
            let Some(event) = mpv.wait_event(0.) else {
                return;
            };
            match event {
                Ok(Event::PlaybackRestart)
                    if self.phase == Phase::Loading
                        && !mpv.get_property::<bool>("pause").unwrap_or(true) =>
                {
                    self.phase = Phase::Started;
                    self.queue.started();
                }
                Ok(Event::EndFile(libmpv::mpv_end_file_reason::Eof))
                    if self.phase == Phase::Started =>
                {
                    let _ = self.advance(true);
                }
                Ok(Event::EndFile(libmpv::mpv_end_file_reason::Eof))
                    if self.phase == Phase::Loading =>
                {
                    if self.fail("A faixa terminou antes de iniciar.".into()) {
                        self.start(true);
                    }
                }
                Err(e) if matches!(self.phase, Phase::Started | Phase::Loading) => {
                    if self.fail(e.to_string()) {
                        self.start(true);
                    }
                }
                _ => {}
            }
        }
    }
    fn command(&mut self, command: PlaybackCommand) -> Result<(), String> {
        match command {
            PlaybackCommand::Select(id) => {
                self.queue.select(id)?;
                self.start(false);
                Ok(())
            }
            PlaybackCommand::PlayPause
                if matches!(self.phase, Phase::Failed | Phase::Ended | Phase::Idle) =>
            {
                let id = self
                    .queue
                    .selected
                    .or_else(|| self.queue.entries.first().map(|e| e.id))
                    .ok_or("Fila vazia.")?;
                self.queue.select(id)?;
                self.start(false);
                Ok(())
            }
            PlaybackCommand::PlayPause if self.phase == Phase::Loading => Ok(()),
            PlaybackCommand::PlayPause => {
                let mpv = self.mpv()?;
                mpv.get_property::<bool>("pause")
                    .and_then(|paused| mpv.set_property("pause", !paused))
                    .map_err(|e| e.to_string())
            }
            PlaybackCommand::Previous => {
                if !self.queue.shuffle
                    && self.mpv()?.get_property::<f64>("time-pos").unwrap_or(0.) > 3.
                {
                    return self
                        .mpv()?
                        .command("seek", &["0", "absolute"])
                        .map_err(|e| e.to_string());
                }
                if let Some(id) = self.queue.previous() {
                    self.queue.select(id)?;
                    self.start(false);
                }
                Ok(())
            }
            PlaybackCommand::Next => self.advance(false),
            PlaybackCommand::Shuffle => {
                self.queue.shuffle = !self.queue.shuffle;
                Ok(())
            }
            PlaybackCommand::Repeat => {
                self.queue.repeat = self.queue.repeat.cycle();
                Ok(())
            }
            PlaybackCommand::SeekPercent(value) => self
                .mpv()?
                .command(
                    "seek",
                    &[&value.clamp(0., 100.).to_string(), "absolute-percent"],
                )
                .map_err(|e| e.to_string()),
            PlaybackCommand::Volume(value) => self
                .mpv()?
                .set_property("volume", f64::from(value.clamp(0., 100.)))
                .map_err(|e| e.to_string()),
            PlaybackCommand::SeekBack => self
                .mpv()?
                .command("seek", &["-10", "relative"])
                .map_err(|error| error.to_string()),
            PlaybackCommand::SeekForward => self
                .mpv()?
                .command("seek", &["10", "relative"])
                .map_err(|error| error.to_string()),
            PlaybackCommand::VolumeDown | PlaybackCommand::VolumeUp => {
                let volume = self
                    .mpv()?
                    .get_property::<f64>("volume")
                    .map_err(|error| error.to_string())?;
                let volume = if matches!(command, PlaybackCommand::VolumeUp) {
                    (volume + 5.0).min(100.0)
                } else {
                    (volume - 5.0).max(0.0)
                };
                self.mpv()?
                    .set_property("volume", volume)
                    .map_err(|error| error.to_string())
            }
        }
    }
    fn publish(&mut self, state: &Mutex<PlaybackState>) {
        let Ok(mut s) = state.lock() else { return };
        s.connected = self.engine.is_ok();
        s.current_index = self.queue.index();
        if s.queue != self.queue.entries {
            s.queue.clone_from(&self.queue.entries);
        }
        s.shuffle = self.queue.shuffle;
        s.repeat = self.queue.repeat.label().into();
        s.loading = self.phase == Phase::Loading;
        s.retry = self.phase == Phase::Failed;
        s.notice = self
            .notice
            .clone()
            .or_else(|| self.engine.as_ref().err().cloned());
        let item = s.current_index.map(|i| &self.queue.entries[i].item);
        s.title = item.map_or("", |i| &i.title).into();
        s.artist = item.map_or("", |i| &i.artist).into();
        let mpv = self.engine.as_ref().ok();
        s.playing = self.phase == Phase::Started
            && mpv.and_then(|m| m.get_property::<bool>("pause").ok()) == Some(false);
        s.position = if self.phase == Phase::Started {
            mpv.and_then(|m| m.get_property::<f64>("time-pos").ok())
                .map_or(0, |p| (p * 1e6) as i64)
        } else {
            0
        };
        s.duration = mpv
            .filter(|_| self.phase == Phase::Started)
            .and_then(|m| m.get_property::<f64>("duration").ok())
            .map_or_else(|| item.map_or(0, |i| i.duration), |d| (d * 1e6) as i64);
        self.volume = mpv
            .and_then(|m| m.get_property::<f64>("volume").ok())
            .unwrap_or(self.volume);
        s.volume = self.volume / 100.;
    }
}
fn run_player(receiver: mpsc::Receiver<Request>, state: Arc<Mutex<PlaybackState>>) {
    let engine = engine();
    if let Ok(mpv) = &engine {
        let _ = mpv.set_property("volume", 50.0);
    }
    let mut worker = Worker {
        queue: Queue::default(),
        engine,
        phase: Phase::Idle,
        automatic: false,
        notice: None,
        volume: 50.,
    };
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Request::Play(items, index, reply)) => {
                worker.queue.replace(items, index);
                worker.start(false);
                let _ = reply.send(Ok(()));
            }
            Ok(Request::Queue(command, reply)) => {
                let _ = reply.send(worker.queue.edit(command));
            }
            Ok(Request::Command(command, reply)) => {
                let _ = reply.send(worker.command(command));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        worker.poll();
        worker.publish(&state);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "consulta o YouTube e reproduz áudio pelo libmpv"]
    fn youtube_search_and_play() {
        let tracks = crate::catalog::Source::Youtube.search("M4rkim").unwrap();
        let track = &tracks[0];
        play_tracks(
            vec![PlaybackItem {
                track_id: track.id.clone(),
                title: track.name.clone(),
                artist: track.artist_names().into(),
                duration: track.duration_micros(),
            }],
            0,
        )
        .unwrap();
        for _ in 0..120 {
            let state = read_state().unwrap();
            if state.position >= 1_000_000 && state.playing {
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
        panic!("O relógio do libmpv não chegou a um segundo");
    }

    #[test]
    fn explicit_decoder_failure_can_retry_without_advancing() {
        let item = PlaybackItem {
            track_id: "abc".to_string().try_into().unwrap(),
            title: "Faixa B".into(),
            artist: String::new(),
            duration: 0,
        };
        let mut worker = Worker {
            queue: Queue::default(),
            engine: engine(),
            phase: Phase::Loading,
            automatic: false,
            notice: None,
            volume: 0.,
        };
        worker.queue.replace(vec![item.clone(), item], 0);
        load(worker.mpv().unwrap(), "/dev/null").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while worker.phase == Phase::Loading && std::time::Instant::now() < deadline {
            worker.poll();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(worker.phase == Phase::Failed);
        assert!(worker.notice.as_deref().unwrap().contains("Faixa B"));
        assert_eq!(worker.queue.index(), Some(0));
        worker.command(PlaybackCommand::PlayPause).unwrap();
        assert!(worker.phase == Phase::Loading);
        assert_eq!(worker.queue.index(), Some(0));
    }
    #[test]
    fn selecting_a_track_clears_pause() {
        let mpv = Mpv::new().unwrap();
        mpv.set_property("ao", "null").unwrap();
        mpv.set_property("pause", true).unwrap();
        load(&mpv, "av://lavfi:anullsrc").unwrap();
        assert!(!mpv.get_property::<bool>("pause").unwrap());
    }
}
