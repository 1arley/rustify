use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use crate::catalog::TrackId;
use crate::queue::{Entry, Queue};
use libmpv::Mpv;
use libmpv::events::Event;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub(crate) struct PlaybackState {
    pub connected: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub artwork_url: Option<String>,
    pub position: i64,
    pub duration: i64,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: String,
    pub queue: Vec<Entry>,
    pub next_index: Option<usize>,
    pub loading: bool,
    pub retry: bool,
    pub notice: Option<String>,
    pub current_index: Option<usize>,
    pub suggesting: bool,
    pub seed_title: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            connected: false,
            playing: false,
            title: String::new(),
            artist: String::new(),
            artwork_url: None,
            position: 0,
            duration: 0,
            volume: 0.5,
            shuffle: false,
            repeat: "None".to_owned(),
            queue: Vec::new(),
            next_index: None,
            loading: false,
            retry: false,
            notice: None,
            current_index: None,
            suggesting: false,
            seed_title: None,
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
    RefreshSuggestions,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct PlaybackItem {
    pub track_id: TrackId,
    pub title: String,
    pub artist: String,
    pub artwork_url: Option<String>,
    pub duration: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct PlaybackSession {
    pub item: PlaybackItem,
    pub position: i64,
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
    Suggestions {
        generation: u64,
        items: Result<Vec<PlaybackItem>, String>,
    },
    Prepared {
        generation: u64,
        revision: u64,
        entry_id: u64,
        result: Result<String, String>,
    },
    IdentityResolved {
        generation: u64,
        entry_id: u64,
        result: Result<String, String>,
    },
    CurrentStreamResolved {
        stream_generation: u64,
        entry_id: u64,
        fallback_url: String,
        result: Result<String, String>,
    },
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
        let worker_sender = sender.clone();
        let startup_error = thread::Builder::new()
            .name("rea-audio".to_owned())
            .spawn(move || run_player(receiver, worker_sender, worker_state))
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
        init.set_option("loop-file", "no")?;
        init.set_option("loop-playlist", "no")?;
        init.set_option("ytdl-format", "ba/b")?;
        init.set_option("ytdl", true)
    })
    .map_err(|e| e.to_string())
}
fn standby_engine(url: &str, volume: f64) -> Result<Mpv, String> {
    let mpv = Mpv::with_initializer(|init| {
        init.set_option("vid", "no")?;
        init.set_option("loop-file", "no")?;
        init.set_option("loop-playlist", "no")?;
        init.set_option("pause", true)?;
        init.set_option("cache", true)?;
        init.set_option("cache-on-disk", false)?;
        init.set_option("cache-secs", 5)?;
        init.set_option("demuxer-cache-wait", true)
    })
    .map_err(|error| error.to_string())?;
    mpv.set_property("volume", volume)
        .map_err(|error| error.to_string())?;
    mpv.command("loadfile", &[url, "replace"])
        .map_err(|error| error.to_string())?;
    Ok(mpv)
}

#[derive(Debug, PartialEq, Eq)]
enum RestoreFailure {
    Permanent(String),
    Temporary(String),
}

fn permanent_ytdlp_failure(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "video unavailable",
        "video is unavailable",
        "private video",
        "video has been removed",
        "video does not exist",
    ]
    .iter()
    .any(|message| error.contains(message))
}

fn spotify_resolution_failure(error: String) -> RestoreFailure {
    if error.contains("Faixa sem título ou artista") {
        RestoreFailure::Permanent(error)
    } else {
        RestoreFailure::Temporary(error)
    }
}

fn restore_stream_url(session: &PlaybackSession) -> Result<String, RestoreFailure> {
    let yt_id = match &session.item.track_id {
        TrackId::Youtube(id) => id.clone(),
        TrackId::Spotify(key) => match crate::library::lookup_resolved_youtube_id(key) {
            Some(id) => id,
            None => {
                let id = crate::catalog::resolve_spotify_to_youtube(
                    &session.item.title,
                    &session.item.artist,
                    session.item.duration,
                )
                .map_err(spotify_resolution_failure)?;
                crate::library::record_resolved_youtube_id(key, &id);
                id
            }
        },
        TrackId::Audius(_) => return Ok(session.item.track_id.stream_url()),
    };
    crate::catalog::youtube_media_url(&yt_id).map_err(|error| {
        if permanent_ytdlp_failure(&error) {
            RestoreFailure::Permanent(error)
        } else {
            RestoreFailure::Temporary(error)
        }
    })
}

fn validate_restored_stream(url: &str, position: i64) -> Result<Mpv, RestoreFailure> {
    let mpv = standby_engine(url, 50.).map_err(RestoreFailure::Temporary)?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        match mpv.wait_event(0.1) {
            Some(Ok(Event::FileLoaded)) => {
                if position > 0 {
                    let seconds = (position as f64 / 1_000_000.0).to_string();
                    mpv.command("seek", &[&seconds, "absolute"])
                        .map_err(|error| RestoreFailure::Temporary(error.to_string()))?;
                }
                return Ok(mpv);
            }
            Some(Ok(Event::EndFile(reason))) => {
                return Err(RestoreFailure::Temporary(format!(
                    "O stream salvo terminou antes de carregar ({reason:?})."
                )));
            }
            // libmpv exposes only a generic loading error here, so keep the saved session.
            Some(Err(error)) => return Err(RestoreFailure::Temporary(error.to_string())),
            _ => {}
        }
    }
    Err(RestoreFailure::Temporary(
        "Tempo esgotado ao validar o stream salvo.".to_owned(),
    ))
}

fn restore_session_with(
    session: Option<PlaybackSession>,
    validate: impl FnOnce(&PlaybackSession) -> Result<Mpv, RestoreFailure>,
) -> Result<Option<(PlaybackSession, Mpv)>, RestoreFailure> {
    session
        .map(|session| validate(&session).map(|mpv| (session, mpv)))
        .transpose()
}
fn load(mpv: &Mpv, url: &str) -> Result<(), String> {
    mpv.command("loadfile", &[url, "replace"])
        .and_then(|_| mpv.set_property("pause", false))
        .map_err(|e| e.to_string())
}

fn accepts_preparation(
    generation: u64,
    revision: u64,
    entry_id: u64,
    current_generation: u64,
    current_revision: u64,
    planned_next: Option<(u64, u64)>,
    playing: bool,
) -> bool {
    playing
        && generation == current_generation
        && revision == current_revision
        && planned_next == Some((revision, entry_id))
}

fn accepts_current_stream(
    stream_generation: u64,
    entry_id: u64,
    current_stream_generation: u64,
    selected: Option<u64>,
    phase: Phase,
) -> bool {
    stream_generation == current_stream_generation
        && selected == Some(entry_id)
        && phase == Phase::Loading
}

fn loading_end_file_error(reason: libmpv::EndFileReason) -> Option<&'static str> {
    match reason {
        libmpv::mpv_end_file_reason::Eof => Some("A faixa terminou antes de iniciar."),
        libmpv::mpv_end_file_reason::Error => Some("Falha ao carregar a faixa."),
        _ => None,
    }
}

fn preloaded_next(
    ready: bool,
    preloaded: Option<(u64, u64)>,
    revision: u64,
    queue: &Queue,
) -> Option<u64> {
    if !ready {
        return None;
    }
    preloaded
        .filter(|(prepared_revision, id)| {
            *prepared_revision == revision && queue.entries.iter().any(|entry| entry.id == *id)
        })
        .map(|(_, id)| id)
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
    sender: mpsc::Sender<Request>,
    state: Arc<Mutex<PlaybackState>>,
    generation: u64,
    suggesting: bool,
    exhausted_at_len: Option<usize>,
    resume_position: i64,
    pending_seek: Option<i64>,
    last_persisted_session: Option<PlaybackSession>,
    last_session_save: Instant,
    queue_revision: u64,
    prepare_generation: u64,
    preparing: bool,
    planned_next: Option<(u64, u64)>,
    preparation_cancel: Option<Arc<AtomicBool>>,
    stream_cancel: Option<Arc<AtomicBool>>,
    preloaded: Option<(u64, u64)>,
    preloader: Option<Mpv>,
    preloader_ready: bool,
    preloader_started: Option<Instant>,
    direct_preloaded: bool,
    identity_resolving: bool,
    stream_resolving: bool,
    stream_generation: u64,
    stream_fallback: Option<String>,
    resolved_identity: Option<(String, String)>,
    loading_since: Option<Instant>,
}
impl Worker {
    fn maybe_request_suggestions(&mut self) {
        if self.suggesting
            || self.queue.repeat != crate::queue::Repeat::Off
            || self.queue.upcoming_count() >= 3
            || self.exhausted_at_len == Some(self.queue.entries.len())
        {
            return;
        }
        let Some(seed) = self.queue.seed.clone().or_else(|| {
            self.queue
                .index()
                .map(|i| self.queue.entries[i].item.clone())
        }) else {
            return;
        };
        let exclude = self.queue.seen_tracks();
        self.suggesting = true;
        let generation = self.generation;
        let sender = self.sender.clone();
        thread::spawn(move || {
            let items = crate::catalog::recommend_tracks(
                &seed.track_id,
                &seed.title,
                &seed.artist,
                seed.duration,
                &exclude,
            )
            .map(|tracks| {
                tracks
                    .into_iter()
                    .map(|track| PlaybackItem {
                        track_id: track.id.clone(),
                        title: track.name.clone(),
                        artist: track.artist_names().to_owned(),
                        artwork_url: track.artwork_url(),
                        duration: track.duration_micros(),
                    })
                    .collect()
            });
            let _ = sender.send(Request::Suggestions { generation, items });
        });
    }
    fn start(&mut self, automatic: bool) {
        self.prepare_generation += 1;
        self.stream_generation += 1;
        if let Some(cancelled) = &self.preparation_cancel {
            cancelled.store(true, Ordering::Release);
        }
        self.preparation_cancel = None;
        self.preparing = false;
        if let Some(cancelled) = &self.stream_cancel {
            cancelled.store(true, Ordering::Release);
        }
        self.stream_cancel = None;
        self.stream_resolving = false;
        self.stream_fallback = None;
        self.planned_next = None;
        self.preloaded = None;
        self.preloader = None;
        self.preloader_ready = false;
        self.preloader_started = None;
        self.direct_preloaded = false;
        self.automatic = automatic;
        if !automatic {
            self.notice = None;
        }
        loop {
            let Some(index) = self.queue.index() else {
                return;
            };
            // Each selection owns its event queue, including when a URL is selected twice.
            if self.phase != Phase::Idle || self.engine.is_err() {
                let volume = self.volume;
                self.engine = engine().and_then(|mpv| {
                    mpv.set_property("volume", volume)
                        .map(|_| mpv)
                        .map_err(|error| error.to_string())
                });
            }
            self.phase = Phase::Loading;
            self.loading_since = Some(Instant::now());
            self.publish();

            let mut stream_track_id = self.queue.entries[index].item.track_id.clone();
            if let TrackId::Spotify(sp_key) = stream_track_id.clone() {
                let title = self.queue.entries[index].item.title.clone();
                let artist = self.queue.entries[index].item.artist.clone();
                let duration = self.queue.entries[index].item.duration;

                let yt_id = self
                    .resolved_identity
                    .as_ref()
                    .filter(|(key, _)| key == &sp_key)
                    .map(|(_, id)| id.clone())
                    .or_else(|| crate::library::lookup_resolved_youtube_id(&sp_key));
                let Some(yt_id) = yt_id else {
                    if self.identity_resolving {
                        return;
                    }
                    let generation = self.prepare_generation;
                    let entry_id = self.queue.entries[index].id;
                    let sender = self.sender.clone();
                    self.identity_resolving = true;
                    thread::spawn(move || {
                        let result =
                            crate::catalog::resolve_spotify_to_youtube(&title, &artist, duration);
                        if let Ok(yt_id) = &result {
                            crate::library::record_resolved_youtube_id(&sp_key, yt_id);
                        }
                        let _ = sender.send(Request::IdentityResolved {
                            generation,
                            entry_id,
                            result,
                        });
                    });
                    return;
                };

                stream_track_id = TrackId::Youtube(yt_id);
            }

            if let TrackId::Youtube(video_id) = &stream_track_id {
                let stream_generation = self.stream_generation;
                let entry_id = self.queue.entries[index].id;
                let fallback_url = stream_track_id.stream_url();
                let video_id = video_id.clone();
                let sender = self.sender.clone();
                let cancelled = Arc::new(AtomicBool::new(false));
                self.stream_cancel = Some(cancelled.clone());
                self.stream_resolving = true;
                thread::spawn(move || {
                    let result = crate::catalog::youtube_media_url_cancellable(
                        &video_id,
                        cancelled.as_ref(),
                    );
                    let _ = sender.send(Request::CurrentStreamResolved {
                        stream_generation,
                        entry_id,
                        fallback_url,
                        result,
                    });
                });
                return;
            }

            let stream_url = stream_track_id.stream_url();
            match self.load_stream(&stream_url) {
                Ok(()) => {
                    self.pending_seek = (self.resume_position > 0).then_some(self.resume_position);
                    self.resume_position = 0;
                    return;
                }
                Err(e) => {
                    if !self.fail(e) {
                        return;
                    }
                }
            }
        }
    }
    fn load_stream(&self, url: &str) -> Result<(), String> {
        let mpv = self.engine.as_ref().map_err(Clone::clone)?;
        mpv.set_property("volume", self.volume)
            .map_err(|error| error.to_string())?;
        let opened_at = Instant::now();
        load(mpv, url)?;
        eprintln!(
            "playback loadfile-call: {} ms",
            opened_at.elapsed().as_millis()
        );
        Ok(())
    }
    fn prepare_next(&mut self) {
        if self.preparing
            || self.phase != Phase::Started
            || self.preloader.is_some()
            || self.planned_next.is_some()
        {
            return;
        }
        let Some(entry_id) = self.queue.next(true) else {
            self.planned_next = None;
            return;
        };
        let Some(entry) = self.queue.entries.iter().find(|entry| entry.id == entry_id) else {
            return;
        };
        let item = entry.item.clone();
        let revision = self.queue_revision;
        self.planned_next = Some((revision, entry_id));
        let generation = self.prepare_generation;
        let sender = self.sender.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.preparation_cancel = Some(cancelled.clone());
        self.preparing = true;
        thread::spawn(move || {
            let cancel = cancelled.as_ref();
            let result = (|| {
                let video_id = match item.track_id {
                    TrackId::Youtube(id) => id,
                    TrackId::Spotify(key) => {
                        let id = match crate::library::lookup_resolved_youtube_id(&key) {
                            Some(id) => id,
                            None => crate::catalog::resolve_spotify_to_youtube_cancellable(
                                &item.title,
                                &item.artist,
                                item.duration,
                                cancel,
                            )?,
                        };
                        crate::library::record_resolved_youtube_id(&key, &id);
                        id
                    }
                    TrackId::Audius(_) => {
                        return Err(
                            "Formato de áudio não suportado pelo pré-carregamento.".to_owned()
                        );
                    }
                };
                crate::catalog::youtube_media_url_cancellable(&video_id, cancel)
            })();
            let _ = sender.send(Request::Prepared {
                generation,
                revision,
                entry_id,
                result,
            });
        });
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
    fn retry_source_stream(&mut self, error: &str) -> bool {
        let Some(url) = self.stream_fallback.take() else {
            return false;
        };
        eprintln!("playback direct stream failed; retrying source URL: {error}");
        match self.load_stream(&url) {
            Ok(()) => {
                self.pending_seek = (self.resume_position > 0).then_some(self.resume_position);
                self.resume_position = 0;
            }
            Err(error) => {
                if self.fail(error) {
                    self.start(true);
                }
            }
        }
        true
    }
    fn invalidate_preload(&mut self) {
        self.prepare_generation += 1;
        if let Some(cancelled) = &self.preparation_cancel {
            cancelled.store(true, Ordering::Release);
        }
        self.preparation_cancel = None;
        self.preparing = false;
        self.planned_next = None;
        self.preloaded = None;
        self.preloader = None;
        self.preloader_ready = false;
        self.preloader_started = None;
    }
    fn advance(&mut self, eof: bool) -> Result<(), String> {
        self.queue.completed();
        let preloaded = preloaded_next(
            self.preloader_ready,
            eof.then(|| self.preloaded.take()).flatten(),
            self.queue_revision,
            &self.queue,
        );
        if let Some(next) = preloaded {
            self.resume_position = 0;
            self.pending_seek = None;
            self.queue.selected = Some(next);
            self.phase = Phase::Loading;
            self.loading_since = Some(Instant::now());
            self.preloader_ready = false;
            self.planned_next = None;
            self.direct_preloaded = true;
            self.automatic = true;
            if let Some(preloader) = self.preloader.take() {
                self.engine = Ok(preloader);
                let mpv = self.mpv()?;
                if mpv
                    .set_property("volume", self.volume)
                    .and_then(|()| mpv.set_property("pause", false))
                    .is_err()
                {
                    self.start(true);
                }
            } else {
                self.start(true);
            }
            eprintln!("playback transition: prepared entry {next}");
        } else if let Some(next) = self.queue.next(eof) {
            self.resume_position = 0;
            self.pending_seek = None;
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
        self.maybe_request_suggestions();
        Ok(())
    }
    fn mpv(&self) -> Result<&Mpv, String> {
        self.engine.as_ref().map_err(Clone::clone)
    }
    fn poll_preloader(&mut self) {
        loop {
            let event = self.preloader.as_ref().and_then(|mpv| mpv.wait_event(0.));
            let status = match event {
                Some(Ok(Event::FileLoaded)) => 1,
                Some(Ok(Event::EndFile(_))) => 2,
                Some(Err(error)) => {
                    eprintln!("playback standby failed: {error}");
                    2
                }
                Some(_) => continue,
                None => return,
            };
            if status == 1 {
                self.preloader_ready = true;
                if let Some(started) = self.preloader_started.take() {
                    eprintln!(
                        "playback standby-ready: {} ms",
                        started.elapsed().as_millis()
                    );
                }
                return;
            }
            self.preloader = None;
            self.preloader_ready = false;
            self.preloader_started = None;
            self.preloaded = None;
            return;
        }
    }
    fn poll(&mut self) {
        self.poll_preloader();
        loop {
            let Some(event) = self.mpv().ok().and_then(|mpv| mpv.wait_event(0.)) else {
                return;
            };
            match event {
                Ok(Event::PlaybackRestart) if self.phase == Phase::Loading => {
                    let paused = self
                        .mpv()
                        .ok()
                        .and_then(|mpv| mpv.get_property::<bool>("pause").ok())
                        .unwrap_or(true);
                    if !paused {
                        self.stream_fallback = None;
                        self.phase = Phase::Started;
                        if let Some(started_at) = self.loading_since.take() {
                            let elapsed = started_at.elapsed().as_millis();
                            if self.direct_preloaded {
                                eprintln!("playback transition-start: {elapsed} ms");
                            } else {
                                eprintln!("playback stream-start: {elapsed} ms");
                            }
                        }
                        self.queue.started();
                        if let Some(position) = self.pending_seek.take() {
                            let seconds = (position as f64 / 1_000_000.0).to_string();
                            if let Ok(mpv) = self.mpv() {
                                let _ = mpv.command("seek", &[&seconds, "absolute"]);
                            }
                        }
                        self.planned_next = None;
                        self.prepare_next();
                    }
                }
                Ok(Event::EndFile(libmpv::mpv_end_file_reason::Eof))
                    if self.phase == Phase::Started =>
                {
                    let _ = self.advance(true);
                }
                Ok(Event::EndFile(reason))
                    if reason != libmpv::mpv_end_file_reason::Eof
                        && self.phase == Phase::Started
                        && self.direct_preloaded =>
                {
                    eprintln!("playback promoted stream ended unexpectedly: {reason:?}");
                    if self.fail(format!("O fluxo de áudio foi interrompido: {reason:?}")) {
                        self.start(true);
                    }
                }
                Ok(Event::EndFile(reason))
                    if self.phase == Phase::Loading && !self.direct_preloaded =>
                {
                    if let Some(error) = loading_end_file_error(reason)
                        && !self.retry_source_stream(error)
                        && self.fail(error.to_owned())
                    {
                        self.start(true);
                    }
                }
                Ok(Event::EndFile(reason))
                    if self.phase == Phase::Loading && self.direct_preloaded =>
                {
                    eprintln!("playback preloaded stream failed to start: {reason:?}");
                    if self.fail(format!("O fluxo de áudio não iniciou: {reason:?}")) {
                        self.start(true);
                    }
                }
                Err(e) if matches!(self.phase, Phase::Started | Phase::Loading) => {
                    if self.direct_preloaded && self.phase == Phase::Loading {
                        let error = e.to_string();
                        eprintln!("playback preloaded stream error: {error}");
                        if self.fail(error) {
                            self.start(true);
                        }
                    } else {
                        let error = e.to_string();
                        eprintln!("playback stream error: {error}");
                        if self.phase == Phase::Loading && self.retry_source_stream(&error) {
                            continue;
                        }
                        if self.fail(error) {
                            self.start(true);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn command(&mut self, command: PlaybackCommand) -> Result<(), String> {
        match command {
            PlaybackCommand::Select(id) => {
                self.queue_revision += 1;
                self.resume_position = 0;
                self.pending_seek = None;
                self.queue.select(id)?;
                self.start(false);
                self.maybe_request_suggestions();
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
                    self.resume_position = 0;
                    self.pending_seek = None;
                    self.queue.select(id)?;
                    self.start(false);
                }
                Ok(())
            }
            PlaybackCommand::Next => self.advance(false),
            PlaybackCommand::Shuffle => {
                self.queue_revision += 1;
                self.invalidate_preload();
                self.queue.shuffle = !self.queue.shuffle;
                self.prepare_next();
                Ok(())
            }
            PlaybackCommand::Repeat => {
                self.queue_revision += 1;
                self.invalidate_preload();
                self.queue.repeat = self.queue.repeat.cycle();
                self.maybe_request_suggestions();
                self.prepare_next();
                Ok(())
            }
            PlaybackCommand::RefreshSuggestions => {
                self.generation += 1;
                self.queue_revision += 1;
                self.invalidate_preload();
                self.suggesting = false;
                self.exhausted_at_len = None;
                self.queue.clear_upcoming_suggestions();
                self.maybe_request_suggestions();
                self.prepare_next();
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
    fn publish(&mut self) {
        let Ok(mut s) = self.state.lock() else { return };
        s.connected = self.engine.is_ok();
        s.current_index = self.queue.index();
        if s.queue != self.queue.entries {
            s.queue.clone_from(&self.queue.entries);
        }
        s.next_index = self.queue.preview_next_index();
        s.shuffle = self.queue.shuffle;
        s.repeat = self.queue.repeat.label().into();
        s.suggesting = self.suggesting;
        s.seed_title = self.queue.seed.as_ref().map(|seed| seed.title.clone());
        s.loading = self.phase == Phase::Loading;
        s.retry = self.phase == Phase::Failed;
        s.notice = self
            .notice
            .clone()
            .or_else(|| self.engine.as_ref().err().cloned());
        let item = s.current_index.map(|i| &self.queue.entries[i].item);
        s.title = item.map_or("", |i| &i.title).into();
        s.artist = item.map_or("", |i| &i.artist).into();
        s.artwork_url = item.and_then(|i| i.artwork_url.clone());
        let mpv = self.engine.as_ref().ok();
        s.playing = self.phase == Phase::Started
            && mpv.and_then(|m| m.get_property::<bool>("pause").ok()) == Some(false);
        s.position = if self.phase == Phase::Started {
            mpv.and_then(|m| m.get_property::<f64>("time-pos").ok())
                .map_or(0, |p| (p * 1e6) as i64)
        } else {
            self.resume_position
        };
        s.duration = mpv
            .filter(|_| self.phase == Phase::Started)
            .and_then(|m| m.get_property::<f64>("duration").ok())
            .map_or_else(|| item.map_or(0, |i| i.duration), |d| (d * 1e6) as i64);
        self.volume = mpv
            .and_then(|m| m.get_property::<f64>("volume").ok())
            .unwrap_or(self.volume);
        s.volume = self.volume / 100.;
        let session = persistable_session(self.phase, &self.queue, s.position);
        drop(s);
        if let Some(session) = session {
            let changed_track = self
                .last_persisted_session
                .as_ref()
                .is_none_or(|saved| saved.item != session.item);
            let moved = self
                .last_persisted_session
                .as_ref()
                .is_none_or(|saved| (saved.position - session.position).abs() >= 5_000_000);
            if changed_track
                || (moved && self.last_session_save.elapsed() >= Duration::from_secs(5))
            {
                self.last_session_save = Instant::now();
                if crate::library::save_playback_session(&session).is_ok() {
                    self.last_persisted_session = Some(session);
                }
            }
        }
    }
}

fn persistable_session(phase: Phase, queue: &Queue, position: i64) -> Option<PlaybackSession> {
    (phase == Phase::Started)
        .then(|| queue.index())
        .flatten()
        .map(|index| PlaybackSession {
            item: queue.entries[index].item.clone(),
            position: position.max(0),
        })
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(cancelled) = &self.preparation_cancel {
            cancelled.store(true, Ordering::Release);
        }
        if let Some(cancelled) = &self.stream_cancel {
            cancelled.store(true, Ordering::Release);
        }
    }
}
fn run_player(
    receiver: mpsc::Receiver<Request>,
    sender: mpsc::Sender<Request>,
    state: Arc<Mutex<PlaybackState>>,
) {
    let engine = engine();
    if let Ok(mpv) = &engine {
        let _ = mpv.set_property("volume", 50.0);
    }
    let saved = crate::library::load_playback_session()
        .ok()
        .flatten()
        .filter(|session| session.position >= 0);
    let restored = restore_session_with(saved, |session| {
        let url = restore_stream_url(session)?;
        validate_restored_stream(&url, session.position)
    });
    let mut queue = Queue::default();
    let (engine, resume_position, restored_session) = match restored {
        Ok(Some((session, mpv))) => (Ok(mpv), session.position, Some(session)),
        Ok(None) => (engine, 0, None),
        Err(RestoreFailure::Permanent(error)) => {
            eprintln!("playback restore unavailable: {error}");
            let _ = crate::library::clear_playback_session();
            (engine, 0, None)
        }
        Err(RestoreFailure::Temporary(error)) => {
            eprintln!("playback restore deferred: {error}");
            (engine, 0, None)
        }
    };
    if let Some(session) = &restored_session {
        queue.replace(vec![session.item.clone()], 0);
    }
    let mut worker = Worker {
        queue,
        engine,
        phase: Phase::Idle,
        automatic: false,
        notice: None,
        volume: 50.,
        sender,
        state,
        generation: 0,
        suggesting: false,
        exhausted_at_len: None,
        resume_position,
        pending_seek: None,
        last_persisted_session: restored_session,
        last_session_save: Instant::now(),
        queue_revision: 0,
        prepare_generation: 0,
        preparing: false,
        planned_next: None,
        preparation_cancel: None,
        stream_cancel: None,
        preloaded: None,
        preloader: None,
        preloader_ready: false,
        preloader_started: None,
        direct_preloaded: false,
        identity_resolving: false,
        stream_resolving: false,
        stream_generation: 0,
        stream_fallback: None,
        resolved_identity: None,
        loading_since: None,
    };
    worker.publish();
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Request::Play(items, index, reply)) => {
                worker.generation += 1;
                worker.queue_revision += 1;
                worker.invalidate_preload();
                worker.suggesting = false;
                worker.exhausted_at_len = None;
                worker.resume_position = 0;
                worker.pending_seek = None;
                worker.queue.replace(items, index);
                let _ = reply.send(Ok(()));
                worker.start(false);
                worker.maybe_request_suggestions();
            }
            Ok(Request::Queue(command, reply)) => {
                let res = worker.queue.edit(command);
                if res.is_ok() {
                    worker.queue_revision += 1;
                    worker.invalidate_preload();
                    worker.prepare_next();
                    worker.maybe_request_suggestions();
                }
                let _ = reply.send(res);
            }
            Ok(Request::Command(command, reply)) => {
                let _ = reply.send(worker.command(command));
            }
            Ok(Request::Suggestions { generation, items }) => {
                if generation == worker.generation {
                    worker.suggesting = false;
                    match items {
                        Ok(items) => {
                            let added = worker.queue.append_suggestions(items);
                            if added == 0 {
                                worker.exhausted_at_len = Some(worker.queue.entries.len());
                            } else {
                                worker.queue_revision += 1;
                                worker.invalidate_preload();
                                worker.exhausted_at_len = None;
                                worker.prepare_next();
                                if worker.phase == Phase::Ended {
                                    let _ = worker.advance(true);
                                }
                            }
                        }
                        Err(_) => {
                            worker.exhausted_at_len = Some(worker.queue.entries.len());
                        }
                    }
                }
            }
            Ok(Request::Prepared {
                generation,
                revision,
                entry_id,
                result,
            }) => {
                if generation == worker.prepare_generation {
                    worker.preparing = false;
                    worker.preparation_cancel = None;
                }
                if accepts_preparation(
                    generation,
                    revision,
                    entry_id,
                    worker.prepare_generation,
                    worker.queue_revision,
                    worker.planned_next,
                    worker.phase == Phase::Started,
                ) {
                    match result {
                        Ok(url) => match standby_engine(&url, worker.volume) {
                            Ok(mpv) => {
                                worker.preloaded = Some((revision, entry_id));
                                worker.preloader = Some(mpv);
                                worker.preloader_ready = false;
                                worker.preloader_started = Some(Instant::now());
                            }
                            Err(error) => eprintln!("playback standby setup failed: {error}"),
                        },
                        Err(error) => eprintln!("playback prefetch failed: {error}"),
                    }
                }
                worker.prepare_next();
            }
            Ok(Request::IdentityResolved {
                generation,
                entry_id,
                result,
            }) => {
                worker.identity_resolving = false;
                if generation == worker.prepare_generation
                    && worker.queue.selected == Some(entry_id)
                    && worker.phase == Phase::Loading
                {
                    match result {
                        Ok(yt_id) => {
                            if let Some(index) = worker.queue.index()
                                && let TrackId::Spotify(key) =
                                    &worker.queue.entries[index].item.track_id
                            {
                                worker.resolved_identity = Some((key.clone(), yt_id));
                            }
                            let automatic = worker.automatic;
                            worker.start(automatic);
                        }
                        Err(error) => {
                            if worker.fail(error) {
                                worker.start(true);
                            }
                        }
                    }
                } else if worker.phase == Phase::Loading
                    && !worker.stream_resolving
                    && worker.queue.index().is_some_and(|index| {
                        matches!(
                            worker.queue.entries[index].item.track_id,
                            TrackId::Spotify(_)
                        )
                    })
                {
                    let automatic = worker.automatic;
                    worker.start(automatic);
                }
            }
            Ok(Request::CurrentStreamResolved {
                stream_generation,
                entry_id,
                fallback_url,
                result,
            }) => {
                if accepts_current_stream(
                    stream_generation,
                    entry_id,
                    worker.stream_generation,
                    worker.queue.selected,
                    worker.phase,
                ) {
                    worker.stream_resolving = false;
                    worker.stream_cancel = None;
                    let url = match result {
                        Ok(url) => {
                            worker.stream_fallback = Some(fallback_url);
                            url
                        }
                        Err(error) => {
                            eprintln!("playback direct stream failed; using MPV ytdl: {error}");
                            worker.stream_fallback = None;
                            fallback_url
                        }
                    };
                    match worker.load_stream(&url) {
                        Ok(()) => {
                            worker.pending_seek =
                                (worker.resume_position > 0).then_some(worker.resume_position);
                            worker.resume_position = 0;
                        }
                        Err(error) => {
                            if !worker.retry_source_stream(&error) && worker.fail(error) {
                                worker.start(true);
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        worker.poll();
        worker.publish();
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // Headless CI runners have no audio device, and mpv aborts or crashes when
    // it fails to open one. Tests only care about the stream state machine, so
    // route audio to mpv's null driver.
    fn test_engine() -> Result<Mpv, String> {
        Mpv::with_initializer(|init| {
            init.set_option("vid", "no")?;
            init.set_option("ao", "null")?;
            init.set_option("loop-file", "no")?;
            init.set_option("loop-playlist", "no")?;
            init.set_option("ytdl-format", "ba/b")?;
            init.set_option("ytdl", true)
        })
        .map_err(|e| e.to_string())
    }

    fn test_standby_engine(url: &str, volume: f64) -> Result<Mpv, String> {
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("vid", "no")?;
            init.set_option("ao", "null")?;
            init.set_option("loop-file", "no")?;
            init.set_option("loop-playlist", "no")?;
            init.set_option("pause", true)?;
            init.set_option("cache", true)?;
            init.set_option("cache-on-disk", false)?;
            init.set_option("cache-secs", 5)?;
            init.set_option("demuxer-cache-wait", true)
        })
        .map_err(|error| error.to_string())?;
        mpv.set_property("volume", volume)
            .map_err(|error| error.to_string())?;
        mpv.command("loadfile", &[url, "replace"])
            .map_err(|error| error.to_string())?;
        Ok(mpv)
    }

    fn worker_for_test() -> Worker {
        Worker {
            queue: Queue::default(),
            engine: test_engine(),
            phase: Phase::Started,
            automatic: false,
            notice: None,
            volume: 0.,
            sender: mpsc::channel().0,
            state: Arc::new(Mutex::new(PlaybackState::default())),
            generation: 0,
            suggesting: false,
            exhausted_at_len: None,
            resume_position: 0,
            pending_seek: None,
            last_persisted_session: None,
            last_session_save: Instant::now(),
            queue_revision: 0,
            prepare_generation: 0,
            preparing: false,
            planned_next: None,
            preparation_cancel: None,
            stream_cancel: None,
            preloaded: None,
            preloader: None,
            preloader_ready: false,
            preloader_started: None,
            direct_preloaded: false,
            identity_resolving: false,
            stream_resolving: false,
            stream_generation: 0,
            stream_fallback: None,
            resolved_identity: None,
            loading_since: None,
        }
    }

    fn test_item(id: &str) -> PlaybackItem {
        PlaybackItem {
            track_id: id.to_owned().try_into().unwrap(),
            title: id.into(),
            artist: String::new(),
            artwork_url: None,
            duration: 1_000_000,
        }
    }

    fn test_session() -> PlaybackSession {
        PlaybackSession {
            item: test_item("abc"),
            position: 12_000_000,
        }
    }

    #[test]
    fn restoring_a_valid_session_keeps_its_identity_and_position() {
        let session = test_session();
        let (restored, mpv) = restore_session_with(Some(session.clone()), |_| {
            validate_restored_stream("av://lavfi:anullsrc=d=60", session.position)
        })
        .unwrap()
        .unwrap();

        assert_eq!(restored, session);
        assert_eq!(restored.position, 12_000_000);
        assert!(mpv.get_property::<bool>("pause").unwrap());
    }

    #[test]
    fn spotify_restore_keeps_spotify_identity_after_youtube_resolution() {
        let item = PlaybackItem {
            track_id: "sp:0123456789ABCDEFGHIJKL".to_owned().try_into().unwrap(),
            ..test_item("abc")
        };
        let session = PlaybackSession { item, position: 0 };
        let (restored, _) = restore_session_with(Some(session.clone()), |candidate| {
            assert_eq!(candidate.item.track_id, session.item.track_id);
            validate_restored_stream("av://lavfi:anullsrc=d=60", candidate.position)
        })
        .unwrap()
        .unwrap();

        assert_eq!(restored.item.track_id, session.item.track_id);
    }

    #[test]
    fn permanently_missing_track_leaves_restoration_empty() {
        let result = restore_session_with(Some(test_session()), |_| {
            Err(RestoreFailure::Permanent("vídeo removido".into()))
        });
        assert!(matches!(result, Err(RestoreFailure::Permanent(_))));
    }

    #[test]
    fn spotify_search_miss_preserves_the_saved_session_as_temporary() {
        assert!(matches!(
            spotify_resolution_failure(
                "Não foi possível encontrar áudio correspondente no YouTube.".into()
            ),
            RestoreFailure::Temporary(_)
        ));
        assert!(matches!(
            spotify_resolution_failure("yt-dlp network timeout".into()),
            RestoreFailure::Temporary(_)
        ));
    }

    #[test]
    fn failed_stream_open_leaves_restoration_empty() {
        let missing =
            std::env::temp_dir().join(format!("rustify-restore-missing-{}", rand::random::<u64>()));
        let result = restore_session_with(Some(test_session()), |_| {
            validate_restored_stream(missing.to_string_lossy().as_ref(), 0)
        });
        assert!(matches!(result, Err(RestoreFailure::Temporary(_))));
    }

    #[test]
    fn no_previous_session_does_not_validate_a_track() {
        let result = restore_session_with(None, |_| panic!("no session to restore"));
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn non_started_selection_cannot_replace_the_saved_session() {
        let mut queue = Queue::default();
        queue.replace(vec![test_item("abc")], 0);
        for phase in [Phase::Idle, Phase::Loading, Phase::Failed] {
            assert!(persistable_session(phase, &queue, 12_000_000).is_none());
        }
        assert_eq!(
            persistable_session(Phase::Started, &queue, 12_000_000).unwrap(),
            test_session()
        );
    }

    #[test]
    fn youtube_errors_distinguish_permanent_unavailability_from_network_failure() {
        assert!(permanent_ytdlp_failure("ERROR: Video unavailable"));
        assert!(permanent_ytdlp_failure("ERROR: Video has been removed"));
        assert!(!permanent_ytdlp_failure("ERROR: No video formats found"));
        assert!(!permanent_ytdlp_failure(
            "Unable to download webpage: timeout"
        ));
    }

    fn wait_for_file_loaded(mpv: &Mpv) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            match mpv.wait_event(0.05) {
                Some(Ok(Event::FileLoaded)) => return,
                Some(Err(error)) => panic!("preloaded stream failed: {error}"),
                _ => {}
            }
        }
        panic!("standby mpv did not load the stream in time");
    }

    #[test]
    fn stale_prefetches_are_rejected_after_selection_or_queue_changes() {
        assert!(accepts_preparation(4, 8, 11, 4, 8, Some((8, 11)), true));
        assert!(!accepts_preparation(3, 8, 11, 4, 8, Some((8, 11)), true));
        assert!(!accepts_preparation(4, 7, 11, 4, 8, Some((8, 11)), true));
        assert!(!accepts_preparation(4, 8, 12, 4, 8, Some((8, 11)), true));
        assert!(!accepts_preparation(4, 8, 11, 4, 8, Some((8, 11)), false));
    }

    #[test]
    fn transition_uses_the_preselected_queue_entry_and_discards_stale_urls() {
        let item = PlaybackItem {
            track_id: "abc".to_string().try_into().unwrap(),
            title: "A".into(),
            artist: String::new(),
            artwork_url: None,
            duration: 0,
        };
        let mut queue = Queue::default();
        queue.replace(vec![item.clone(), item], 0);
        let next = queue.entries[1].id;

        assert_eq!(preloaded_next(true, Some((5, next)), 5, &queue), Some(next));
        assert_eq!(preloaded_next(true, Some((4, next)), 5, &queue), None);
        assert_eq!(preloaded_next(false, Some((5, next)), 5, &queue), None);
        queue.entries.remove(1);
        assert_eq!(preloaded_next(true, Some((5, next)), 5, &queue), None);
    }

    #[test]
    fn invalidating_preload_does_not_cancel_the_current_stream() {
        let mut worker = worker_for_test();
        let current_stream = Arc::new(AtomicBool::new(false));
        let next_stream = Arc::new(AtomicBool::new(false));
        worker.stream_cancel = Some(current_stream.clone());
        worker.preparation_cancel = Some(next_stream.clone());
        worker.stream_resolving = true;
        worker.preparing = true;
        let generation = worker.stream_generation;

        worker.invalidate_preload();

        assert!(next_stream.load(Ordering::Acquire));
        assert!(!worker.preparing);
        assert!(!current_stream.load(Ordering::Acquire));
        assert!(worker.stream_resolving);
        assert_eq!(worker.stream_generation, generation);
    }

    #[test]
    fn stale_current_stream_results_are_ignored() {
        let generation = 7;
        let selected = Some(42);

        assert!(accepts_current_stream(
            generation,
            42,
            generation,
            selected,
            Phase::Loading
        ));
        assert!(!accepts_current_stream(
            generation - 1,
            42,
            generation,
            selected,
            Phase::Loading
        ));
        assert!(!accepts_current_stream(
            generation,
            43,
            generation,
            selected,
            Phase::Loading
        ));
        assert!(!accepts_current_stream(
            generation,
            42,
            generation,
            selected,
            Phase::Started
        ));
    }

    #[test]
    fn loading_end_file_errors_fail_the_track_but_stop_events_are_ignored() {
        assert_eq!(
            loading_end_file_error(libmpv::mpv_end_file_reason::Error),
            Some("Falha ao carregar a faixa.")
        );
        assert_eq!(
            loading_end_file_error(libmpv::mpv_end_file_reason::Stop),
            None
        );
    }

    #[test]
    fn queue_invalidation_allows_preparing_the_revised_next_entry() {
        let mut worker = worker_for_test();
        let mut current = test_item("abc");
        let mut next = test_item("def");
        current.track_id = TrackId::Audius("current".into());
        next.track_id = TrackId::Audius("next".into());
        worker.queue.replace(vec![current, next], 0);
        let next_id = worker.queue.entries[1].id;
        worker.planned_next = Some((0, next_id));
        worker.preloaded = Some((0, next_id));

        worker.queue_revision += 1;
        worker.invalidate_preload();
        worker.prepare_next();

        assert_eq!(worker.planned_next, Some((worker.queue_revision, next_id)));
        assert!(worker.preparing);
    }

    #[test]
    fn refreshing_suggestions_replans_a_remaining_manual_next_entry() {
        let mut worker = worker_for_test();
        let mut current = test_item("abc");
        let mut next = test_item("def");
        current.track_id = TrackId::Audius("current".into());
        next.track_id = TrackId::Audius("next".into());
        worker.queue.replace(vec![current, next], 0);
        worker.queue.repeat = crate::queue::Repeat::Playlist;
        let next_id = worker.queue.entries[1].id;

        worker.command(PlaybackCommand::RefreshSuggestions).unwrap();

        assert_eq!(worker.planned_next, Some((worker.queue_revision, next_id)));
        assert!(worker.preparing);
    }

    #[test]
    fn prepared_stream_is_loaded_paused_then_promoted_for_transition() {
        let mut worker = worker_for_test();
        worker
            .queue
            .replace(vec![test_item("abc"), test_item("def")], 0);
        worker.exhausted_at_len = Some(worker.queue.entries.len());
        let next = worker.queue.entries[1].id;
        let standby = test_standby_engine("av://lavfi:anullsrc=d=1", 0.).unwrap();
        wait_for_file_loaded(&standby);
        assert!(standby.get_property::<bool>("pause").unwrap());

        worker.preloaded = Some((worker.queue_revision, next));
        worker.preloader_ready = true;
        worker.preloader = Some(standby);
        worker.advance(true).unwrap();
        assert_eq!(worker.queue.selected, Some(next));

        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.phase == Phase::Loading && Instant::now() < deadline {
            worker.poll();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(worker.phase == Phase::Started);
    }

    #[test]
    fn failed_promoted_track_keeps_automatic_queue_advance() {
        let mut worker = worker_for_test();
        worker.queue.replace(
            vec![test_item("abc"), test_item("def"), test_item("ghi")],
            0,
        );
        worker.exhausted_at_len = Some(worker.queue.entries.len());
        let next = worker.queue.entries[1].id;
        let standby = test_standby_engine("av://lavfi:anullsrc=d=1", 0.).unwrap();
        wait_for_file_loaded(&standby);
        worker.preloaded = Some((worker.queue_revision, next));
        worker.preloader_ready = true;
        worker.preloader = Some(standby);

        worker.advance(true).unwrap();
        assert!(worker.automatic);
        assert!(worker.fail("preloaded stream failed".into()));
        assert_eq!(worker.queue.selected, Some(worker.queue.entries[2].id));
    }

    #[test]
    fn midtrack_failure_of_a_promoted_stream_does_not_restart_the_track() {
        let mut worker = worker_for_test();
        let mut current = test_item("abc");
        let mut next = test_item("def");
        current.track_id = TrackId::Audius("current".into());
        next.track_id = TrackId::Audius("next".into());
        worker.queue.replace(vec![current, next], 0);
        worker.exhausted_at_len = Some(worker.queue.entries.len());
        let next_id = worker.queue.entries[1].id;
        let standby = test_standby_engine("av://lavfi:anullsrc=d=10", 0.).unwrap();
        wait_for_file_loaded(&standby);
        worker.preloaded = Some((worker.queue_revision, next_id));
        worker.preloader_ready = true;
        worker.preloader = Some(standby);
        worker.advance(true).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.phase == Phase::Loading && Instant::now() < deadline {
            worker.poll();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(worker.phase == Phase::Started);
        assert!(worker.direct_preloaded);

        worker.mpv().unwrap().command("stop", &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while worker.phase == Phase::Started && Instant::now() < deadline {
            worker.poll();
            thread::sleep(Duration::from_millis(10));
        }

        assert!(worker.phase == Phase::Failed);
        assert_eq!(worker.queue.selected, Some(next_id));
        assert!(worker.notice.is_some());
        assert!(worker.automatic);
    }

    #[test]
    fn failed_standby_stream_leaves_current_playback_running() {
        let mut worker = worker_for_test();
        worker
            .queue
            .replace(vec![test_item("abc"), test_item("def")], 0);
        worker.preloaded = Some((worker.queue_revision, worker.queue.entries[1].id));
        let missing =
            std::env::temp_dir().join(format!("rustify-missing-{}", rand::random::<u64>()));
        worker.preloader =
            Some(test_standby_engine(missing.to_string_lossy().as_ref(), 0.).unwrap());

        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.preloader.is_some() && Instant::now() < deadline {
            worker.poll_preloader();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(worker.preloader.is_none());
        assert!(worker.phase == Phase::Started);
        assert_eq!(worker.queue.index(), Some(0));
    }

    #[test]
    #[ignore = "consulta o YouTube e abre uma URL temporária pelo libmpv"]
    fn youtube_standby_loads_the_direct_stream_while_paused() {
        let url = crate::catalog::youtube_media_url("5NV6Rdv1a3I").unwrap();
        let mpv = test_standby_engine(&url, 0.).unwrap();
        wait_for_file_loaded(&mpv);
        assert!(mpv.get_property::<bool>("pause").unwrap());
    }

    #[test]
    #[ignore = "consulta o YouTube e reproduz áudio pelo libmpv"]
    fn youtube_search_and_play() {
        let tracks = crate::catalog::search_all("M4rkim").unwrap();
        let track = &tracks[0];
        play_tracks(
            vec![PlaybackItem {
                track_id: track.id.clone(),
                title: track.name.clone(),
                artist: track.artist_names().into(),
                artwork_url: track.artwork_url(),
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
        panic!(
            "O relógio do libmpv não chegou a um segundo: {:#?}",
            read_state()
        );
    }

    #[test]
    fn explicit_decoder_failure_can_retry_without_advancing() {
        let item = PlaybackItem {
            track_id: "abc".to_string().try_into().unwrap(),
            title: "Faixa B".into(),
            artist: String::new(),
            artwork_url: None,
            duration: 0,
        };
        let mut worker = Worker {
            queue: Queue::default(),
            engine: test_engine(),
            phase: Phase::Loading,
            automatic: false,
            notice: None,
            volume: 0.,
            sender: mpsc::channel().0,
            state: Arc::new(Mutex::new(PlaybackState::default())),
            generation: 0,
            suggesting: false,
            exhausted_at_len: None,
            resume_position: 0,
            pending_seek: None,
            last_persisted_session: None,
            last_session_save: Instant::now(),
            queue_revision: 0,
            prepare_generation: 0,
            preparing: false,
            planned_next: None,
            preparation_cancel: None,
            stream_cancel: None,
            preloaded: None,
            preloader: None,
            preloader_ready: false,
            preloader_started: None,
            direct_preloaded: false,
            identity_resolving: false,
            stream_resolving: false,
            stream_generation: 0,
            stream_fallback: None,
            resolved_identity: None,
            loading_since: None,
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
