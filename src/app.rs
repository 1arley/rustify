use std::sync::Arc;
use std::time::Duration;

use crate::catalog::{self, Track, TrackId};
use crate::components::{Assets, Http, artwork, header, icon_heart, status_card, svg_icon};
use crate::design::{
    ACCENT, BLACK, CARD, CARD_HOVER, ERROR, INFO, MUTED, PRIMARY, SURFACE, cover_color,
};
use crate::home::playback_status;
use crate::library::{self, SavedPlaylist, SpotifySnapshot};
use crate::playback::{self, PlaybackCommand, PlaybackItem, PlaybackState, QueueCommand};
use crate::spotify;
use crate::youtube;
use gpui::{
    AnyElement, App, Application, Bounds, Context, Render, Subscription, Timer, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::{
    Disableable, Root,
    button::{Button, ButtonVariants},
};

fn playback_item(track: &Track) -> PlaybackItem {
    PlaybackItem {
        track_id: track.id.clone(),
        title: track.name.clone(),
        artist: track.artist_names().to_owned(),
        artwork_url: track.artwork_url(),
        duration: track.duration_micros(),
    }
}

fn pick_file_dialog() -> Result<Option<String>, String> {
    Ok(rfd::FileDialog::new()
        .set_title("Selecionar exportação do Spotify")
        .add_filter("Exportações do Spotify", &["json", "csv"])
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActivePage {
    Home,
    Search,
    NowPlaying,
    Library,
    Playlist,
    Import,
    Queue,
}

struct Home {
    seek: gpui::Entity<SliderState>,
    volume: gpui::Entity<SliderState>,
    playback: PlaybackState,
    search_input: gpui::Entity<InputState>,
    spotify_url_input: gpui::Entity<InputState>,
    youtube_url_input: gpui::Entity<InputState>,
    spotify_file_input: gpui::Entity<InputState>,
    client_id_input: gpui::Entity<InputState>,
    playlist_name_input: gpui::Entity<InputState>,
    _search_subscription: Subscription,
    _url_subscription: Subscription,
    _youtube_url_subscription: Subscription,
    _file_subscription: Subscription,
    results: Vec<Track>,
    active_page: ActivePage,
    liked_tracks: Vec<Track>,
    playlists: Vec<SavedPlaylist>,
    local_liked_tracks: Vec<Track>,
    local_playlists: Vec<SavedPlaylist>,
    selected_playlist: Option<String>,
    playlist_picker_track: Option<TrackId>,
    client_id: String,
    spotify_connected: bool,
    library_loaded: bool,
    library_error: Option<String>,
    searching: bool,
    importing: bool,
    import_notice: Option<(String, bool)>,
    pending_file_path: Option<String>,
    confirm_legacy_cleanup: bool,
    message: Option<String>,
}

impl Home {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (liked_tracks, likes_ok, likes_err) = match library::load_liked_tracks() {
            Ok(tracks) => (tracks, true, None),
            Err(error) => (Vec::new(), false, Some(error)),
        };
        let (playlists, playlists_ok, playlists_err) = match library::load_playlists() {
            Ok(playlists) => (playlists, true, None),
            Err(error) => (Vec::new(), false, Some(error)),
        };
        let (local_liked_tracks, local_likes_ok, local_likes_err) =
            match library::load_local_liked_tracks() {
                Ok(tracks) => (tracks, true, None),
                Err(error) => (Vec::new(), false, Some(error)),
            };
        let (local_playlists, local_playlists_ok, local_playlists_err) =
            match library::load_local_playlists() {
                Ok(playlists) => (playlists, true, None),
                Err(error) => (Vec::new(), false, Some(error)),
            };
        let library_loaded = likes_ok && playlists_ok && local_likes_ok && local_playlists_ok;
        let library_error = likes_err
            .or(playlists_err)
            .or(local_likes_err)
            .or(local_playlists_err);
        let client_id = spotify::client_id().unwrap_or_default();
        let spotify_connected = spotify::is_connected(&client_id);

        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("O que você quer ouvir? (ou cole um link)")
        });
        let _search_subscription = cx.subscribe(&search_input, |this, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.search(cx);
            }
        });

        let spotify_url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://open.spotify.com/playlist/... ou spotify:playlist:...")
        });
        let _url_subscription = cx.subscribe(&spotify_url_input, |this, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.import_spotify_url(None, cx);
            }
        });

        let youtube_url_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("https://www.youtube.com/playlist?list=...")
        });
        let _youtube_url_subscription = cx.subscribe(&youtube_url_input, |this, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.import_youtube_playlist(cx);
            }
        });

        let spotify_file_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("~/Downloads/Playlist1.json, YourLibrary.json, arquivo .csv ou pasta")
        });
        let _file_subscription = cx.subscribe(&spotify_file_input, |this, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.import_spotify_path(None, cx);
            }
        });

        let client_id_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Client ID do Spotify Developer Dashboard")
                .default_value(client_id.clone())
        });
        let playlist_name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Nome da playlist"));

        let seek = cx.new(|_| SliderState::new().max(100.).step(0.1));
        let volume = cx.new(|_| SliderState::new().max(100.).default_value(50.));
        for (slider, command) in [
            (
                &seek,
                PlaybackCommand::SeekPercent as fn(f32) -> PlaybackCommand,
            ),
            (&volume, PlaybackCommand::Volume),
        ] {
            cx.subscribe(slider, move |this, _, SliderEvent::Change(value), cx| {
                this.run_command(command(value.start()), cx)
            })
            .detach();
        }
        Self {
            seek,
            volume,
            playback: PlaybackState::default(),
            search_input,
            spotify_url_input,
            youtube_url_input,
            spotify_file_input,
            client_id_input,
            playlist_name_input,
            _search_subscription,
            _url_subscription,
            _youtube_url_subscription,
            _file_subscription,
            results: Vec::new(),
            active_page: ActivePage::Home,
            liked_tracks,
            playlists,
            local_liked_tracks,
            local_playlists,
            selected_playlist: None,
            playlist_picker_track: None,
            client_id,
            spotify_connected,
            library_loaded,
            library_error,
            searching: false,
            importing: false,
            import_notice: None,
            pending_file_path: None,
            confirm_legacy_cleanup: false,
            message: None,
        }
    }

    fn start_polling(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                let state = cx
                    .background_executor()
                    .spawn(async { playback::read_state() })
                    .await;
                if this
                    .update(cx, |view, cx| {
                        match state {
                            Ok(state) => {
                                view.playback = state;
                            }
                            Err(_) => {
                                view.playback = PlaybackState::default();
                            }
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                Timer::after(Duration::from_secs(1)).await;
            }
        })
        .detach();
    }

    fn run_command(&mut self, command: PlaybackCommand, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { playback::run(command) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |view, cx| {
                    view.message = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().trim().to_owned();
        if !query.is_empty() && spotify::is_spotify_url_or_uri(&query) {
            self.import_spotify_url(Some(query), cx);
            return;
        }
        self.active_page = ActivePage::Search;
        if self.searching {
            return;
        }
        if query.is_empty() {
            cx.notify();
            return;
        }

        self.searching = true;
        self.results.clear();
        self.message = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { catalog::search_all(&query) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.searching = false;
                match result {
                    Ok(results) => {
                        view.results = results;
                        if view.results.is_empty() {
                            view.message =
                                Some("Nenhuma faixa reproduzível encontrada.".to_owned());
                        }
                    }
                    Err(error) => view.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn play_track(&mut self, queue: &[Track], selected: usize, cx: &mut Context<Self>) {
        let items = queue.iter().map(playback_item).collect();
        self.play_items(items, selected, cx);
    }

    fn play_items(&mut self, items: Vec<PlaybackItem>, selected: usize, cx: &mut Context<Self>) {
        self.message = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { playback::play_tracks(items, selected) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.message = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    fn queue_action(&mut self, command: QueueCommand, cx: &mut Context<Self>) {
        self.message = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { playback::update_queue(command) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |view, cx| {
                    view.message = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn enqueue_tracks(&mut self, tracks: Vec<Track>, cx: &mut Context<Self>) {
        if tracks.is_empty() {
            return;
        }
        let items: Vec<PlaybackItem> = tracks.iter().map(playback_item).collect();
        self.message = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    for item in items {
                        playback::update_queue(QueueCommand::Enqueue(item))?;
                    }
                    Ok::<(), String>(())
                })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |view, cx| {
                    view.message = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn toggle_liked(&mut self, track: Track, cx: &mut Context<Self>) {
        if self.library_loaded {
            let mut local_tracks = self.local_liked_tracks.clone();
            if local_tracks.iter().any(|liked| liked.same_identity(&track)) {
                local_tracks.retain(|liked| !liked.same_identity(&track));
            } else {
                local_tracks.push(track);
            }
            if let Err(error) = library::save_liked_tracks(&local_tracks) {
                self.library_error = Some(error.clone());
                self.message = Some(error);
            } else if let Err(error) = self.reload_library() {
                self.library_error = Some(error.clone());
                self.message = Some(error);
            }
        } else if self
            .liked_tracks
            .iter()
            .any(|liked| liked.same_identity(&track))
        {
            self.liked_tracks
                .retain(|liked| !liked.same_identity(&track));
        } else {
            self.liked_tracks.push(track);
        }
        cx.notify();
    }

    fn remove_playlist(&mut self, playlist_id: &str, cx: &mut Context<Self>) {
        if !self
            .local_playlists
            .iter()
            .any(|playlist| playlist.id == playlist_id)
        {
            return;
        }
        let mut local_playlists = self.local_playlists.clone();
        local_playlists.retain(|playlist| playlist.id != playlist_id);
        if let Err(error) = library::save_playlists(&local_playlists) {
            self.library_error = Some(error.clone());
            self.message = Some(error);
            cx.notify();
            return;
        }
        if self.selected_playlist.as_deref() == Some(playlist_id) {
            self.selected_playlist = None;
            self.active_page = ActivePage::Home;
        }
        if let Err(error) = self.reload_library() {
            self.library_error = Some(error.clone());
            self.message = Some(error);
        }
        cx.notify();
    }

    fn reload_library(&mut self) -> Result<(), String> {
        let liked_tracks = library::load_liked_tracks()?;
        let playlists = library::load_playlists()?;
        let local_liked_tracks = library::load_local_liked_tracks()?;
        let local_playlists = library::load_local_playlists()?;
        self.liked_tracks = liked_tracks;
        self.playlists = playlists;
        self.local_liked_tracks = local_liked_tracks;
        self.local_playlists = local_playlists;
        self.library_loaded = true;
        self.library_error = None;
        Ok(())
    }

    fn create_local_playlist(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.playlist_name_input.read(cx).value().trim().to_owned();
        match library::create_local_playlist(&name) {
            Ok(_) => {
                self.playlist_name_input.update(cx, |input, cx| {
                    input.set_value("".to_owned(), window, cx);
                });
                if let Err(error) = self.reload_library() {
                    self.message = Some(error);
                }
            }
            Err(error) => self.message = Some(error),
        }
        cx.notify();
    }

    fn add_track_to_playlist(&mut self, playlist_id: &str, track: Track, cx: &mut Context<Self>) {
        match library::add_track_to_local_playlist(playlist_id, track) {
            Ok(_) => {
                if let Err(error) = self.reload_library() {
                    self.message = Some(error);
                }
            }
            Err(error) => self.message = Some(error),
        }
        cx.notify();
    }

    fn save_youtube_playlist(&mut self, playlist: SavedPlaylist, cx: &mut Context<Self>) {
        let id = playlist.id.clone();
        let mut local_playlists = match library::load_local_playlists() {
            Ok(playlists) => playlists,
            Err(error) => {
                self.import_notice = Some((error.clone(), true));
                self.message = Some(error);
                cx.notify();
                return;
            }
        };
        let (_, tracks_added) = library::merge_playlists(&mut local_playlists, vec![playlist]);
        if let Err(error) = library::save_playlists(&local_playlists) {
            self.import_notice = Some((error.clone(), true));
            self.message = Some(error);
            cx.notify();
            return;
        }
        if let Err(error) = self.reload_library() {
            self.import_notice = Some((error.clone(), true));
            self.message = Some(error);
        } else {
            self.message = None;
            self.import_notice = Some((
                format!("Playlist importada: {tracks_added} faixas adicionadas."),
                false,
            ));
            if let Some(playlist) = self.playlists.iter().find(|playlist| playlist.id == id) {
                self.selected_playlist = Some(playlist.id.clone());
                self.active_page = ActivePage::Playlist;
            }
        }
        cx.notify();
    }

    fn apply_import_outcome(
        &mut self,
        outcome: spotify::ImportOutcome,
        open_single: bool,
        cx: &mut Context<Self>,
    ) {
        let target_hint = if open_single && outcome.playlists.len() == 1 {
            Some((
                outcome.playlists[0].id.clone(),
                outcome.playlists[0].name.clone(),
            ))
        } else {
            None
        };
        let incoming_playlists_count = outcome.playlists.len();
        let summary = outcome.summary;
        let mut local_playlists = match library::load_local_playlists() {
            Ok(playlists) => playlists,
            Err(error) => {
                self.import_notice = Some((error.clone(), true));
                self.message = Some(error);
                cx.notify();
                return;
            }
        };
        let mut local_liked_tracks = match library::load_local_liked_tracks() {
            Ok(tracks) => tracks,
            Err(error) => {
                self.import_notice = Some((error.clone(), true));
                self.message = Some(error);
                cx.notify();
                return;
            }
        };
        let (pl_affected, pl_tracks_added) =
            library::merge_playlists(&mut local_playlists, outcome.playlists);
        let liked_added =
            library::merge_liked_tracks(&mut local_liked_tracks, outcome.liked_tracks);

        if let Err(error) = library::save_playlists(&local_playlists) {
            self.import_notice = Some((error.clone(), true));
            self.message = Some(error);
            cx.notify();
            return;
        }
        if liked_added > 0
            && let Err(error) = library::save_liked_tracks(&local_liked_tracks)
        {
            self.import_notice = Some((error.clone(), true));
            self.message = Some(error);
            cx.notify();
            return;
        }

        if let Err(error) = self.reload_library() {
            self.import_notice = Some((error.clone(), true));
            self.message = Some(error);
            cx.notify();
            return;
        }

        self.library_loaded = true;
        self.library_error = None;
        self.message = None;
        let notice = if pl_tracks_added == 0 && liked_added == 0 {
            if incoming_playlists_count > 0 {
                "Todas as faixas dessa importação já estavam salvas na sua biblioteca.".to_owned()
            } else {
                summary
            }
        } else {
            format!(
                "{summary} (+{pl_tracks_added} em playlists, +{liked_added} em curtidas; {pl_affected} playlists atualizadas)"
            )
        };
        self.import_notice = Some((notice, false));

        if let Some((target_id, target_name)) = target_hint
            && let Some(found) = self.playlists.iter().find(|p| {
                p.id == target_id || p.name.trim().eq_ignore_ascii_case(target_name.trim())
            })
        {
            self.selected_playlist = Some(found.id.clone());
            self.active_page = ActivePage::Playlist;
        }
        cx.notify();
    }

    fn import_spotify_url(&mut self, override_url: Option<String>, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        let raw = override_url
            .unwrap_or_else(|| self.spotify_url_input.read(cx).value().trim().to_owned());
        self.importing = true;
        self.import_notice = None;
        self.message = None;
        let client_id = (!self.client_id.trim().is_empty()).then(|| self.client_id.clone());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::import_url_or_uri(&raw, client_id.as_deref()) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.importing = false;
                match result {
                    Ok(outcome) => view.apply_import_outcome(outcome, true, cx),
                    Err(error) => {
                        view.import_notice = Some((error, true));
                        view.active_page = ActivePage::Import;
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn import_youtube_playlist(&mut self, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        let url = self.youtube_url_input.read(cx).value().trim().to_owned();
        if url.is_empty() {
            self.import_notice = Some(("Cole o link de uma playlist do YouTube.".to_owned(), true));
            cx.notify();
            return;
        }
        self.importing = true;
        self.import_notice = Some(("Importando playlist do YouTube…".to_owned(), false));
        self.message = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { youtube::import_playlist(&url) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.importing = false;
                match result {
                    Ok(playlist) => view.save_youtube_playlist(playlist, cx),
                    Err(error) => {
                        view.import_notice = Some((error.clone(), true));
                        view.message = Some(error);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn import_spotify_path(&mut self, override_path: Option<String>, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        let raw = override_path
            .unwrap_or_else(|| self.spotify_file_input.read(cx).value().trim().to_owned());
        self.importing = true;
        self.import_notice = None;
        self.message = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::import_path(&raw) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.importing = false;
                match result {
                    Ok(outcome) => view.apply_import_outcome(outcome, true, cx),
                    Err(error) => {
                        view.import_notice = Some((error, true));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn scan_downloads(&mut self, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        self.importing = true;
        self.import_notice = None;
        self.message = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::scan_downloads() })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.importing = false;
                match result {
                    Ok(outcome) => view.apply_import_outcome(outcome, true, cx),
                    Err(error) => {
                        view.import_notice = Some((error, true));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn pick_import_file(&mut self, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        self.import_notice = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { pick_file_dialog() })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(Some(path)) => {
                    view.pending_file_path = Some(path.clone());
                    view.import_spotify_path(Some(path), cx);
                }
                Ok(None) => {}
                Err(error) => {
                    view.import_notice = Some((error, true));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn save_client_id(&mut self, cx: &mut Context<Self>) {
        let value = self.client_id_input.read(cx).value().to_string();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::save_client_id(&value) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(saved) => {
                        view.spotify_connected = spotify::is_connected(&saved);
                        view.client_id = saved;
                        view.import_notice = Some((
                            "Client ID salvo. Agora conecte e sincronize sua conta Spotify."
                                .to_owned(),
                            false,
                        ));
                    }
                    Err(error) => view.import_notice = Some((error, true)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn sync_spotify_account(&mut self, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        let input_cid = self.client_id_input.read(cx).value().trim().to_owned();
        let cid = if input_cid.is_empty() {
            self.client_id.trim().to_owned()
        } else {
            input_cid
        };
        if cid.is_empty() {
            self.import_notice = Some((
                "Informe seu Client ID do Spotify Developer Dashboard antes de conectar."
                    .to_owned(),
                true,
            ));
            cx.notify();
            return;
        }

        self.importing = true;
        self.import_notice = Some((
            "Aguardando autenticação no navegador e sincronizando biblioteca do Spotify…"
                .to_owned(),
            false,
        ));
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let saved = spotify::save_client_id(&cid)?;
                    let outcome = spotify::sync_account(&saved);
                    let snapshot = outcome
                        .as_ref()
                        .map(|outcome| SpotifySnapshot {
                            liked_tracks: outcome.liked_tracks.clone(),
                            playlists: outcome.playlists.clone(),
                            resolved_tracks: Default::default(),
                        })
                        .map_err(Clone::clone);
                    library::replace_spotify_snapshot(snapshot)?;
                    Ok::<(String, spotify::ImportOutcome), String>((saved, outcome?))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.importing = false;
                match result {
                    Ok((saved, outcome)) => {
                        view.client_id = saved;
                        view.spotify_connected = true;
                        if let Err(error) = view.reload_library() {
                            view.import_notice = Some((error.clone(), true));
                            view.message = Some(error);
                        } else {
                            view.message = None;
                            view.import_notice = Some((outcome.summary, false));
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        view.spotify_connected = spotify::is_connected(&view.client_id);
                        view.import_notice = Some((error, true));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn disconnect_spotify(&mut self, cx: &mut Context<Self>) {
        let cid = self.client_id.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::disconnect(&cid) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(()) => {
                        view.spotify_connected = false;
                        if let Err(error) = view.reload_library() {
                            view.import_notice = Some((error.clone(), true));
                            view.message = Some(error);
                        } else {
                            view.import_notice = Some((
                                "Conta Spotify desconectada deste dispositivo.".to_owned(),
                                false,
                            ));
                        }
                    }
                    Err(error) => {
                        view.spotify_connected = spotify::is_connected(&view.client_id);
                        let notice = match view.reload_library() {
                            Ok(()) => error,
                            Err(load_error) => format!("{error}; {load_error}"),
                        };
                        view.import_notice = Some((notice.clone(), true));
                        view.message = Some(notice);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_legacy_spotify_data(&mut self, cx: &mut Context<Self>) {
        self.confirm_legacy_cleanup = false;
        match library::remove_legacy_spotify_data() {
            Ok((tracks, playlists)) => {
                if let Err(error) = self.reload_library() {
                    self.import_notice = Some((error.clone(), true));
                    self.message = Some(error);
                } else {
                    self.import_notice = Some((
                        format!(
                            "Removidas {tracks} faixas e {playlists} playlists antigas ligadas ao Spotify. Itens sem origem identificável foram preservados."
                        ),
                        false,
                    ));
                }
            }
            Err(error) => {
                let _ = self.reload_library();
                self.import_notice = Some((error.clone(), true));
                self.message = Some(error);
            }
        }
        cx.notify();
    }

    fn content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = match self.active_page {
            ActivePage::Home => self.home_panel(cx),
            ActivePage::Search => self.search_panel(cx),
            ActivePage::NowPlaying => self.now_playing_page(cx),
            ActivePage::Library => self.library_panel(cx),
            ActivePage::Playlist => self.playlist_panel(cx),
            ActivePage::Import => self.import_panel(cx),
            ActivePage::Queue => self.queue_panel(cx),
        };
        div()
            .flex_1()
            .min_h_0()
            .id("home-content")
            .overflow_scroll()
            .rounded_md()
            .bg(rgb(SURFACE))
            .child(panel)
    }

    fn home_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let playlists_section =
            if self.playlists.is_empty() {
                div()
                    .rounded_md()
                    .bg(rgb(CARD))
                    .border_1()
                    .border_color(rgb(CARD_HOVER))
                    .p_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Traga suas playlists para cá"),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(
                                "Cole um link de playlist ou importe um arquivo exportado.",
                            )),
                    )
                    .child(
                        Button::new("home-go-import")
                            .primary()
                            .bg(rgb(PRIMARY))
                            .text_color(rgb(0xffffff))
                            .child(svg_icon("icons/import.svg", 0xffffff, 14.0))
                            .child("Importar playlists")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_page = ActivePage::Import;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            } else {
                let cards = self.playlists.iter().enumerate().map(|(index, playlist)| {
                    let id_open = playlist.id.clone();
                    let id_play = playlist.id.clone();
                    let tracks_for_play = playlist.tracks.clone();
                    let mark: String = if playlist.name.is_empty() {
                        "♫".to_owned()
                    } else {
                        playlist.name.chars().take(2).collect()
                    };
                    div()
                        .id(("home-playlist-card", index))
                        .w(px(160.0))
                        .rounded_md()
                        .bg(rgb(CARD))
                        .border_1()
                        .border_color(rgb(CARD_HOVER))
                        .p_3()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected_playlist = Some(id_open.clone());
                            this.active_page = ActivePage::Playlist;
                            cx.notify();
                        }))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(artwork(
                                    &mark,
                                    cover_color(&playlist.name),
                                    44.0,
                                    playlist.artwork_url.as_deref(),
                                ))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .overflow_hidden()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .truncate()
                                                .child(playlist.name.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(MUTED))
                                                .truncate()
                                                .child(format!("{} faixas", playlist.tracks.len())),
                                        ),
                                ),
                        )
                        .child(
                            Button::new(("home-playlist-play", index))
                                .primary()
                                .bg(rgb(PRIMARY))
                                .text_color(rgb(0xffffff))
                                .compact()
                                .disabled(tracks_for_play.is_empty())
                                .child(svg_icon("icons/play.svg", 0xffffff, 12.0))
                                .tooltip("Tocar playlist")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.selected_playlist = Some(id_play.clone());
                                    this.play_track(&tracks_for_play, 0, cx);
                                })),
                        )
                });

                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(MUTED))
                                    .child(format!("Suas playlists ({})", self.playlists.len())),
                            )
                            .child(
                                Button::new("home-import-more")
                                    .ghost()
                                    .compact()
                                    .text_color(rgb(INFO))
                                    .child("Importar mais")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.active_page = ActivePage::Import;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(div().flex().flex_wrap().gap_3().children(cards))
                    .into_any_element()
            };

        let tracks_for_play: std::rc::Rc<[Track]> = self.liked_tracks.clone().into();
        let liked_tracks = if self.liked_tracks.is_empty() {
            status_card(
                "Nenhuma música curtida ainda",
                "Busque uma faixa e use o coração para salvá-la aqui.",
                "info",
            )
            .into_any_element()
        } else {
            let cards = self.liked_tracks.iter().enumerate().map(|(index, track)| {
                let tracks = tracks_for_play.clone();
                let mark: String = track.name.chars().take(2).collect();
                let artwork_url = track.artwork_url();
                Button::new(("home-liked-track", index))
                    .ghost()
                    .w(px(132.0))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(CARD))
                    .child(artwork(
                        &mark,
                        cover_color(&track.name),
                        112.0,
                        artwork_url.as_deref(),
                    ))
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .truncate()
                            .child(track.name.clone()),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(track.artist_names().to_owned()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.play_track(&tracks, index, cx);
                    }))
            });
            div()
                .w_full()
                .id("home-liked-tracks")
                .overflow_scroll()
                .flex()
                .flex_nowrap()
                .gap_2()
                .children(cards)
                .into_any_element()
        };

        div()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_2xl()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Início"),
            )
            .child(playlists_section)
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(MUTED))
                    .child("Músicas curtidas"),
            )
            .child(liked_tracks)
            .child(self.message_card())
            .into_any_element()
    }

    fn search_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.track_rows(&self.results, cx);
        let content = if self.searching {
            div()
                .text_sm()
                .text_color(rgb(INFO))
                .child("Pesquisando faixas…")
                .into_any_element()
        } else if self.results.is_empty() && self.message.is_none() {
            status_card(
                "Encontre sua próxima música",
                "Pesquise faixas de artistas independentes e toque o áudio aqui no app.",
                "info",
            )
            .into_any_element()
        } else {
            rows.into_any_element()
        };

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_6()
            .py_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(INFO))
                            .child("BUSCAR MÚSICAS"),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Busque e ouça música"),
                    )
                    .child(div().text_sm().text_color(rgb(MUTED)).child(
                        "Busca e reprodução não exigem conta. As faixas abrem no player integrado.",
                    ))
                    .child(
                        Button::new("search-catalogs")
                            .primary()
                            .bg(rgb(PRIMARY))
                            .text_color(rgb(0xffffff))
                            .child(svg_icon("icons/search.svg", 0xffffff, 14.0))
                            .child("Buscar músicas")
                            .on_click(cx.listener(|this, _, _, cx| this.search(cx))),
                    )
                    .child(content)
                    .child(self.message_card()),
            )
            .into_any_element()
    }

    fn library_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = if self.liked_tracks.is_empty() {
            status_card(
                "Sua biblioteca está vazia",
                "Use o coração ao lado de uma faixa para salvá-la neste dispositivo.",
                "info",
            )
            .into_any_element()
        } else {
            self.track_rows(&self.liked_tracks, cx).into_any_element()
        };
        let playlist_rows = self
            .playlists
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, playlist)| {
                let id = playlist.id.clone();
                let mark: String = playlist.name.chars().take(2).collect();
                Button::new(("library-playlist", index))
                    .ghost()
                    .w_full()
                    .flex()
                    .items_center()
                    .p_2()
                    .rounded_md()
                    .bg(rgb(CARD))
                    .cursor_pointer()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(artwork(
                                &mark,
                                cover_color(&playlist.name),
                                40.0,
                                playlist.artwork_url.as_deref(),
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .overflow_hidden()
                                    .child(div().w_full().truncate().child(playlist.name))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(MUTED))
                                            .child(format!("{} faixas", playlist.tracks.len())),
                                    ),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_playlist = Some(id.clone());
                        this.active_page = ActivePage::Playlist;
                        cx.notify();
                    }))
            });
        let playlists = if self.playlists.is_empty() {
            div().into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(MUTED))
                        .child("Playlists"),
                )
                .children(playlist_rows)
                .into_any_element()
        };

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_6()
            .py_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Músicas curtidas"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .child("Curtidas e playlists disponíveis neste dispositivo."),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .h(px(36.0))
                                    .flex()
                                    .items_center()
                                    .px_3()
                                    .rounded_md()
                                    .bg(rgb(SURFACE))
                                    .border_1()
                                    .border_color(rgb(CARD_HOVER))
                                    .child(
                                        Input::new(&self.playlist_name_input)
                                            .appearance(false)
                                            .w_full(),
                                    ),
                            )
                            .child(
                                Button::new("create-local-playlist")
                                    .primary()
                                    .bg(rgb(PRIMARY))
                                    .text_color(rgb(0xffffff))
                                    .child(svg_icon("icons/plus.svg", 0xffffff, 14.0))
                                    .child("Criar playlist")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.create_local_playlist(window, cx);
                                    })),
                            ),
                    )
                    .child(playlists)
                    .child(content)
                    .child(self.message_card()),
            )
            .into_any_element()
    }

    fn now_playing_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let (status_label, status_color) = playback_status(&self.playback);
        let title = if self.playback.title.is_empty() {
            if self.playback.connected {
                "Nada tocando"
            } else {
                "Player de áudio indisponível"
            }
        } else {
            &self.playback.title
        };
        let artist = if self.playback.artist.is_empty() {
            "Escolha uma faixa para começar"
        } else {
            &self.playback.artist
        };
        let mark: String = title.chars().take(2).collect();
        div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .py_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(620.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_5()
                    .child(artwork(
                        &mark,
                        cover_color(title),
                        320.0,
                        self.playback.artwork_url.as_deref(),
                    ))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(rgb(status_color))
                                    .child(status_label),
                            )
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_center()
                                    .truncate()
                                    .child(title.to_owned()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(MUTED))
                                    .child(artist.to_owned()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_5()
                            .child(self.icon_control_button(
                                "icons/shuffle.svg",
                                "now-playing-shuffle",
                                PlaybackCommand::Shuffle,
                                self.playback.shuffle,
                                18.0,
                                cx,
                            ))
                            .child(self.icon_control_button(
                                "icons/prev.svg",
                                "now-playing-prev",
                                PlaybackCommand::Previous,
                                false,
                                18.0,
                                cx,
                            ))
                            .child(
                                Button::new("now-playing-play-pause")
                                    .primary()
                                    .bg(rgb(0xffffff))
                                    .text_color(rgb(BLACK))
                                    .child(svg_icon(
                                        if self.playback.playing {
                                            "icons/pause.svg"
                                        } else {
                                            "icons/play.svg"
                                        },
                                        BLACK,
                                        18.0,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.run_command(PlaybackCommand::PlayPause, cx)
                                    })),
                            )
                            .child(self.icon_control_button(
                                "icons/next.svg",
                                "now-playing-next",
                                PlaybackCommand::Next,
                                false,
                                18.0,
                                cx,
                            ))
                            .child(self.icon_control_button(
                                "icons/repeat.svg",
                                "now-playing-repeat",
                                PlaybackCommand::Repeat,
                                self.playback.repeat != "None",
                                18.0,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child(playback::format_time(self.playback.position))
                            .child(div().flex_1().child(
                                Slider::new(&self.seek).disabled(self.playback.duration <= 0),
                            ))
                            .child(playback::format_time(self.playback.duration)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(svg_icon("icons/volume.svg", INFO, 16.0))
                            .child(div().w(px(220.0)).child(
                                Slider::new(&self.volume).disabled(!self.playback.connected),
                            )),
                    )
                    .child(
                        Button::new("now-playing-open-queue")
                            .ghost()
                            .child(format!("Abrir fila · {} faixas", self.playback.queue.len()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_page = ActivePage::Queue;
                                cx.notify();
                            })),
                    )
                    .child(self.message_card()),
            )
            .into_any_element()
    }

    fn playlist_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(playlist) = self
            .selected_playlist
            .as_deref()
            .and_then(|id| self.playlists.iter().find(|pl| pl.id == id))
        else {
            return div()
                .p_6()
                .flex()
                .flex_col()
                .gap_4()
                .child(status_card(
                    "Nenhuma playlist selecionada",
                    "Escolha uma playlist na barra lateral ou importe uma nova.",
                    "info",
                ))
                .into_any_element();
        };

        let mark: String = if playlist.name.is_empty() {
            "♫".to_owned()
        } else {
            playlist.name.chars().take(2).collect()
        };
        let tracks_for_play = playlist.tracks.clone();
        let tracks_for_queue = playlist.tracks.clone();
        let playlist_id_remove = playlist.id.clone();
        let source_url = playlist.source_url.clone();

        let mut actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                Button::new("playlist-play-all")
                    .primary()
                    .bg(rgb(PRIMARY))
                    .text_color(rgb(0xffffff))
                    .disabled(playlist.tracks.is_empty())
                    .child(svg_icon("icons/play.svg", 0xffffff, 14.0))
                    .child("Tocar playlist")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.play_track(&tracks_for_play, 0, cx);
                    })),
            )
            .child(
                Button::new("playlist-enqueue-all")
                    .bg(rgb(CARD_HOVER))
                    .border_color(rgb(CARD_HOVER))
                    .text_color(rgb(0xffffff))
                    .disabled(playlist.tracks.is_empty())
                    .child(svg_icon("icons/plus.svg", 0xffffff, 14.0))
                    .child("Adicionar à fila")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.enqueue_tracks(tracks_for_queue.clone(), cx);
                    })),
            );

        if let Some(url) = source_url {
            actions = actions.child(
                Button::new("playlist-open-original")
                    .ghost()
                    .text_color(rgb(MUTED))
                    .child(svg_icon("icons/external.svg", MUTED, 14.0))
                    .child("Abrir playlist original")
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
            );
        }

        if self
            .local_playlists
            .iter()
            .any(|local_playlist| local_playlist.id == playlist_id_remove)
        {
            actions = actions.child(
                Button::new("playlist-remove")
                    .ghost()
                    .text_color(rgb(ERROR))
                    .child("Remover playlist")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.remove_playlist(&playlist_id_remove, cx);
                    })),
            );
        }

        let track_list = if playlist.tracks.is_empty() {
            status_card(
                "Playlist vazia",
                "Nenhuma música encontrada nesta playlist.",
                "info",
            )
            .into_any_element()
        } else {
            self.track_rows(&playlist.tracks, cx).into_any_element()
        };

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_6()
            .py_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .rounded_md()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(CARD_HOVER))
                            .p_5()
                            .flex()
                            .items_center()
                            .gap_5()
                            .child(artwork(
                                &mark,
                                cover_color(&playlist.name),
                                88.0,
                                playlist.artwork_url.as_deref(),
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(rgb(ACCENT))
                                            .child("PLAYLIST"),
                                    )
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .truncate()
                                            .child(playlist.name.clone()),
                                    )
                                    .child(div().text_sm().text_color(rgb(MUTED)).truncate().child(
                                        if playlist.description.trim().is_empty() {
                                            format!("{} faixas", playlist.tracks.len())
                                        } else {
                                            format!(
                                                "{} · {} faixas",
                                                playlist.description,
                                                playlist.tracks.len()
                                            )
                                        },
                                    ))
                                    .child(actions),
                            ),
                    )
                    .child(track_list)
                    .child(self.message_card()),
            )
            .into_any_element()
    }

    fn import_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let notice_banner = if self.importing {
            div()
                .rounded_md()
                .bg(rgb(CARD))
                .border_l_4()
                .border_color(rgb(INFO))
                .p_3()
                .text_sm()
                .text_color(rgb(INFO))
                .child("Importando playlists e músicas do Spotify…")
                .into_any_element()
        } else if let Some((text, is_error)) = &self.import_notice {
            div()
                .rounded_md()
                .bg(rgb(CARD))
                .border_l_4()
                .border_color(rgb(if *is_error { ERROR } else { ACCENT }))
                .p_3()
                .text_sm()
                .text_color(rgb(0xffffff))
                .child(text.clone())
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let input_box = |input: &gpui::Entity<InputState>| {
            div()
                .w_full()
                .h(px(36.0))
                .flex()
                .items_center()
                .px_3()
                .rounded_md()
                .bg(rgb(SURFACE))
                .border_1()
                .border_color(rgb(CARD_HOVER))
                .text_sm()
                .child(Input::new(input).appearance(false).w_full())
        };

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_6()
            .py_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(ACCENT))
                            .child("IMPORTAR PLAYLISTS E MÚSICAS"),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Importar músicas e playlists"),
                    )
                    .child(div().text_sm().text_color(rgb(MUTED)).child(
                        "As playlists guardam metadados. O áudio só é aberto no player quando você escolhe uma faixa.",
                    ))
                    .child(notice_banner)
                    .child(
                        div()
                            .rounded_md()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(CARD_HOVER))
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("1. Link ou URI público do Spotify"),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(
                                "Cole o link ou URI de uma playlist, álbum ou faixa pública do Spotify (sem precisar de chave de API).",
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_3()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .child(input_box(&self.spotify_url_input)),
                                    )
                                    .child(
                                        Button::new("import-url-btn")
                                            .primary()
                                            .bg(rgb(PRIMARY))
                                            .text_color(rgb(0xffffff))
                                            .disabled(self.importing)
                                            .child(svg_icon("icons/import.svg", 0xffffff, 14.0))
                                            .child("Importar link")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.import_spotify_url(None, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .rounded_md()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(CARD_HOVER))
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("2. Playlist do YouTube"),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(
                                "Cole o link de uma playlist pública. Importamos até 500 faixas sem baixar os áudios.",
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_3()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .child(input_box(&self.youtube_url_input)),
                                    )
                                    .child(
                                        Button::new("youtube-playlist-import")
                                            .primary()
                                            .bg(rgb(PRIMARY))
                                            .text_color(rgb(0xffffff))
                                            .disabled(self.importing)
                                            .child(svg_icon("icons/import.svg", 0xffffff, 14.0))
                                            .child("Importar playlist")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.import_youtube_playlist(cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .rounded_md()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(CARD_HOVER))
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("3. Arquivos ou pasta exportada (JSON / CSV)"),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(
                                "Aceita Playlist1.json, YourLibrary.json, StreamingHistory*.json, a pasta completa Spotify Account Data / MyData e CSVs do Exportify.",
                            ))
                            .child(input_box(&self.spotify_file_input))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        Button::new("import-path-btn")
                                            .primary()
                                            .bg(rgb(PRIMARY))
                                            .text_color(rgb(0xffffff))
                                            .disabled(self.importing)
                                            .child("Importar caminho")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.import_spotify_path(None, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("import-pick-file-btn")
                                            .bg(rgb(SURFACE))
                                            .border_color(rgb(CARD_HOVER))
                                            .text_color(rgb(0xffffff))
                                            .disabled(self.importing)
                                            .child("Selecionar arquivo…")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.pick_import_file(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("import-scan-downloads-btn")
                                            .bg(rgb(SURFACE))
                                            .border_color(rgb(CARD_HOVER))
                                            .text_color(rgb(INFO))
                                            .disabled(self.importing)
                                            .child("Buscar em ~/Downloads")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.scan_downloads(cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .rounded_md()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(CARD_HOVER))
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_base()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .child("4. Conta Spotify (OAuth PKCE)"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(if self.spotify_connected {
                                                ACCENT
                                            } else {
                                                MUTED
                                            }))
                                            .child(if self.spotify_connected {
                                                "Conectada"
                                            } else {
                                                "Não conectada"
                                            }),
                                    ),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(
                                "Opcional: informe um Client ID do Spotify Developer Dashboard (redirect URI http://127.0.0.1:<porta>/callback) para importar todas as playlists privadas e músicas curtidas da conta.",
                            ))
                            .child(input_box(&self.client_id_input))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        Button::new("spotify-save-cid")
                                            .bg(rgb(SURFACE))
                                            .border_color(rgb(CARD_HOVER))
                                            .text_color(rgb(0xffffff))
                                            .disabled(self.importing)
                                            .child("Salvar Client ID")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.save_client_id(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("spotify-sync-account")
                                            .primary()
                                            .bg(rgb(ACCENT))
                                            .text_color(rgb(BLACK))
                                            .disabled(self.importing)
                                            .child(if self.spotify_connected {
                                                "Sincronizar conta"
                                            } else {
                                                "Conectar e sincronizar conta"
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.sync_spotify_account(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("spotify-disconnect")
                                            .ghost()
                                            .text_color(rgb(MUTED))
                                            .disabled(!self.spotify_connected || self.importing)
                                            .child("Desconectar")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.disconnect_spotify(cx);
                                            })),
                                    )
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(
                                "A sincronização fica separada dos imports locais. A remoção abaixo apaga apenas itens antigos com vínculo Spotify identificável; dados ambíguos são preservados.",
                            ))
                            .child(if self.confirm_legacy_cleanup {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(rgb(ERROR))
                                            .child("Isso apaga faixas e playlists locais identificáveis como imports Spotify e não pode ser desfeito. Itens ambíguos serão mantidos."),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap_2()
                                            .child(
                                                Button::new("spotify-legacy-cleanup-confirm")
                                                    .bg(rgb(ERROR))
                                                    .text_color(rgb(0xffffff))
                                                    .child("Confirmar remoção")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.remove_legacy_spotify_data(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("spotify-legacy-cleanup-cancel")
                                                    .ghost()
                                                    .child("Cancelar")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.confirm_legacy_cleanup = false;
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                    .into_any_element()
                            } else {
                                Button::new("spotify-legacy-cleanup")
                                    .ghost()
                                    .text_color(rgb(ERROR))
                                    .child("Remover imports antigos ligados ao Spotify")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_legacy_cleanup = true;
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            }),
                    )
                    .child(self.message_card()),
            )
            .into_any_element()
    }

    fn queue_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let queue = self.playback.queue.clone();
        let current_index = self.playback.current_index;
        let content = if queue.is_empty() {
            status_card(
                "Sua fila está vazia",
                "Adicione faixas pela busca ou inicie uma reprodução.",
                "info",
            )
            .into_any_element()
        } else {
            let rows = queue.iter().enumerate().map(|(index, item)| {
                let id = item.id;
                let is_suggested = item.suggested;

                let next = queue.get(index + 1).map(|e| e.id);
                let item = &item.item;
                let is_current = current_index == Some(index);
                let can_edit = current_index.map_or(true, |current| index > current);
                let first_upcoming = current_index.map_or(0, |current| current + 1);
                let mark: String = if item.title.is_empty() {
                    "♫".to_owned()
                } else {
                    item.title.chars().take(2).collect()
                };
                let mut controls = div().flex().items_center().gap_1();
                if can_edit {
                    if index > first_upcoming {
                        let previous = queue[index - 1].id;
                        controls = controls.child(
                            Button::new(("queue-up", index))
                                .ghost()
                                .compact()
                                .label("↑")
                                .tooltip("Mover para cima")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.queue_action(
                                        QueueCommand::Move {
                                            from: id,
                                            to: previous,
                                        },
                                        cx,
                                    )
                                })),
                        );
                    }
                    if let Some(next) = next {
                        controls = controls.child(
                            Button::new(("queue-down", index))
                                .ghost()
                                .compact()
                                .label("↓")
                                .tooltip("Mover para baixo")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.queue_action(QueueCommand::Move { from: id, to: next }, cx)
                                })),
                        );
                    }
                    controls = controls.child(
                        Button::new(("queue-remove", index))
                            .ghost()
                            .compact()
                            .label("×")
                            .tooltip("Remover da fila")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.queue_action(QueueCommand::Remove(id), cx)
                            })),
                    );
                }

                div()
                    .w_full()
                    .min_h(px(62.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(if is_current { CARD_HOVER } else { CARD }))
                    .border_l_4()
                    .border_color(rgb(if is_current { ACCENT } else { CARD }))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .w(px(28.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .text_color(rgb(if is_current { ACCENT } else { MUTED }))
                                    .child(if is_current {
                                        svg_icon("icons/music.svg", ACCENT, 16.0).into_any_element()
                                    } else {
                                        (index + 1).to_string().into_any_element()
                                    }),
                            )
                            .child(artwork(
                                &mark,
                                cover_color(&item.title),
                                40.0,
                                item.artwork_url.as_deref(),
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .overflow_hidden()
                                    .child(div().text_sm().truncate().child(item.title.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .truncate()
                                            .text_color(rgb(if is_current {
                                                ACCENT
                                            } else {
                                                MUTED
                                            }))
                                            .child(if is_current {
                                                format!(
                                                    "{} · {}",
                                                    item.artist,
                                                    if self.playback.loading {
                                                        "Carregando"
                                                    } else if self.playback.playing {
                                                        "Tocando"
                                                    } else {
                                                        "Selecionada"
                                                    }
                                                )
                                            } else if is_suggested {
                                                if item.artist.is_empty() {
                                                    "Sugerida".to_owned()
                                                } else {
                                                    format!("{} · Sugerida", item.artist)
                                                }
                                            } else {
                                                item.artist.clone()
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new(("queue-play", index))
                                    .primary()
                                    .bg(rgb(PRIMARY))
                                    .text_color(rgb(0xffffff))
                                    .compact()
                                    .child(svg_icon("icons/play.svg", 0xffffff, 14.0))
                                    .tooltip("Tocar a partir daqui")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run_command(PlaybackCommand::Select(id), cx)
                                    })),
                            )
                            .child(controls),
                    )
            });
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(rows)
                .into_any_element()
        };

        let suggestions_header = if let Some(seed_title) = &self.playback.seed_title {
            let status_text = if self.playback.suggesting {
                format!("Gerando sugestões automáticas com base em \"{seed_title}\"…")
            } else {
                format!("Sugestões automáticas baseadas em \"{seed_title}\"")
            };
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_sm()
                        .truncate()
                        .text_color(rgb(if self.playback.suggesting {
                            INFO
                        } else {
                            MUTED
                        }))
                        .child(status_text),
                )
                .child(
                    Button::new("refresh-suggestions")
                        .bg(rgb(CARD))
                        .border_color(rgb(CARD_HOVER))
                        .text_color(rgb(0xffffff))
                        .compact()
                        .disabled(self.playback.suggesting)
                        .label("Atualizar sugestões")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run_command(PlaybackCommand::RefreshSuggestions, cx)
                        })),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_6()
            .py_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(format!("Fila · {} faixas", queue.len())),
                    )
                    .child(suggestions_header)
                    .child(content)
                    .child(self.message_card()),
            )
            .into_any_element()
    }

    fn track_rows(&self, tracks: &[Track], cx: &mut Context<Self>) -> gpui::Div {
        let queue: std::rc::Rc<[Track]> = tracks.into();
        let play_single_seed = self.active_page == ActivePage::Search;
        let rows = tracks.iter().cloned().enumerate().map(|(index, track)| {
            let is_liked = self
                .local_liked_tracks
                .iter()
                .any(|liked| liked.same_identity(&track));
            let is_account_liked = !is_liked
                && self
                    .liked_tracks
                    .iter()
                    .any(|liked| liked.same_identity(&track));
            let favorite = track.clone();
            let queue_item = playback_item(&track);
            let artwork_url = track.artwork_url();
            let playback_queue = queue.clone();
            let artist = track.artist_names();
            let album = track.album_name().trim();
            let album = if ["youtube", "audius", "spotify"]
                .iter()
                .any(|source| album.eq_ignore_ascii_case(source))
            {
                ""
            } else {
                album
            };
            let base_details = match (artist.is_empty(), album.is_empty()) {
                (true, true) => String::new(),
                (false, true) => artist.to_owned(),
                (true, false) => album.to_owned(),
                (false, false) => format!("{artist} · {album}"),
            };
            let details = if track.duration_micros() > 0 {
                format!(
                    "{base_details} · {}",
                    playback::format_time(track.duration_micros())
                )
            } else {
                base_details
            };
            let mark: String = if track.name.is_empty() {
                "♫".to_owned()
            } else {
                track.name.chars().take(2).collect()
            };
            let cover_bg = cover_color(&track.name);
            let picker_id = track.id.clone();
            let picker_track = track.clone();
            let show_playlist_picker = self.playlist_picker_track.as_ref() == Some(&picker_id);

            let mut actions = div().flex_none().flex().items_center().gap_2().child(
                Button::new(("track-like", index))
                    .ghost()
                    .compact()
                    .disabled(is_account_liked)
                    .child(icon_heart(is_liked))
                    .tooltip(if is_account_liked {
                        "Já está na biblioteca"
                    } else {
                        "Salvar na biblioteca local"
                    })
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_liked(favorite.clone(), cx)),
                    ),
            );
            actions = actions
                .child(
                    Button::new(("track-add-playlist", index))
                        .ghost()
                        .compact()
                        .label("Playlist")
                        .tooltip("Adicionar à playlist")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.playlist_picker_track =
                                if this.playlist_picker_track.as_ref() == Some(&picker_id) {
                                    None
                                } else {
                                    Some(picker_id.clone())
                                };
                            cx.notify();
                        })),
                )
                .child(
                    Button::new(("track-queue", index))
                        .ghost()
                        .compact()
                        .child(svg_icon("icons/plus.svg", MUTED, 14.0))
                        .tooltip("Adicionar à fila")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.queue_action(QueueCommand::Enqueue(queue_item.clone()), cx)
                        })),
                )
                .child(
                    Button::new(("track-play", index))
                        .primary()
                        .bg(rgb(PRIMARY))
                        .text_color(rgb(0xffffff))
                        .compact()
                        .child(svg_icon("icons/play.svg", 0xffffff, 14.0))
                        .tooltip("Tocar no app")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if play_single_seed {
                                this.play_items(vec![playback_item(&playback_queue[index])], 0, cx);
                            } else {
                                this.play_track(&playback_queue, index, cx);
                            }
                        })),
                );

            let row = div()
                .id(("track-row", index))
                .w_full()
                .h(px(66.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .px_4()
                .rounded_md()
                .bg(rgb(CARD))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(artwork(&mark, cover_bg, 44.0, artwork_url.as_deref()))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .overflow_hidden()
                                .child(div().text_sm().truncate().child(track.name))
                                .child(
                                    div()
                                        .text_xs()
                                        .truncate()
                                        .text_color(rgb(MUTED))
                                        .child(details),
                                ),
                        ),
                )
                .child(actions);
            let picker = if show_playlist_picker {
                if self.local_playlists.is_empty() {
                    div()
                        .px_4()
                        .py_2()
                        .child(
                            Button::new(("track-create-playlist", index))
                                .ghost()
                                .label("Crie uma playlist na Biblioteca")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.active_page = ActivePage::Library;
                                    cx.notify();
                                })),
                        )
                        .into_any_element()
                } else {
                    let choices = self
                        .local_playlists
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(playlist_index, playlist)| {
                            let playlist_id = playlist.id.clone();
                            let track = picker_track.clone();
                            let added = playlist
                                .tracks
                                .iter()
                                .any(|saved| saved.same_identity(&track));
                            let label = if added {
                                format!("Adicionada · {}", playlist.name)
                            } else {
                                format!("Adicionar a {}", playlist.name)
                            };
                            Button::new((
                                "track-playlist",
                                index * self.local_playlists.len() + playlist_index,
                            ))
                            .ghost()
                            .compact()
                            .disabled(added)
                            .label(label)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.add_track_to_playlist(&playlist_id, track.clone(), cx);
                                },
                            ))
                        })
                        .collect::<Vec<_>>();
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(SURFACE))
                        .children(choices)
                        .into_any_element()
                }
            } else {
                div().into_any_element()
            };
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(row)
                .child(picker)
        });
        div().flex().flex_col().gap_2().children(rows)
    }

    fn message_card(&self) -> AnyElement {
        if let Some(message) = self
            .playback
            .notice
            .as_ref()
            .or(self.message.as_ref())
            .or(self.library_error.as_ref())
        {
            div()
                .rounded_md()
                .bg(rgb(CARD))
                .border_l_4()
                .border_color(rgb(ERROR))
                .p_3()
                .text_sm()
                .text_color(rgb(0xffffff))
                .child(message.clone())
                .into_any_element()
        } else {
            div().into_any_element()
        }
    }

    fn nav_item(
        &self,
        id: &'static str,
        page: ActivePage,
        icon_path: &'static str,
        label: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.active_page == page;
        let fg = if active { 0xffffff } else { MUTED };
        Button::new(id)
            .ghost()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .bg(rgb(if active { PRIMARY } else { SURFACE }))
            .text_sm()
            .font_weight(if active {
                gpui::FontWeight::SEMIBOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .text_color(rgb(fg))
            .child(svg_icon(icon_path, fg, 16.0))
            .child(div().min_w_0().flex_1().truncate().child(label))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_page = page;
                cx.notify();
            }))
    }

    fn sidebar(&self, compact: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let playlist_rows: Vec<_> = self
            .playlists
            .iter()
            .enumerate()
            .map(|(index, playlist)| {
                let active = self.active_page == ActivePage::Playlist
                    && self.selected_playlist.as_deref() == Some(playlist.id.as_str());
                let id = playlist.id.clone();
                let mark: String = if playlist.name.is_empty() {
                    "♫".to_owned()
                } else {
                    playlist.name.chars().take(2).collect()
                };
                Button::new(("sidebar-playlist", index))
                    .ghost()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(if active { CARD_HOVER } else { SURFACE }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_playlist = Some(id.clone());
                        this.active_page = ActivePage::Playlist;
                        cx.notify();
                    }))
                    .child(artwork(
                        &mark,
                        cover_color(&playlist.name),
                        24.0,
                        playlist.artwork_url.as_deref(),
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_sm()
                                    .truncate()
                                    .text_color(rgb(if active { 0xffffff } else { MUTED }))
                                    .font_weight(if active {
                                        gpui::FontWeight::SEMIBOLD
                                    } else {
                                        gpui::FontWeight::NORMAL
                                    })
                                    .child(playlist.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(rgb(MUTED))
                                    .child(format!("{} faixas", playlist.tracks.len())),
                            ),
                    )
            })
            .collect();
        let playlist_list = if compact {
            div().flex_1().into_any_element()
        } else {
            div()
                .id("sidebar-playlists")
                .flex_1()
                .min_h_0()
                .overflow_scroll()
                .flex()
                .flex_col()
                .gap_1()
                .children(playlist_rows)
                .into_any_element()
        };

        div()
            .w(px(if compact { 196.0 } else { 272.0 }))
            .flex_none()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(rgb(SURFACE))
            .child(self.nav_item(
                "nav-home",
                ActivePage::Home,
                "icons/home.svg",
                "Início".into(),
                cx,
            ))
            .child(self.nav_item(
                "nav-search",
                ActivePage::Search,
                "icons/search.svg",
                "Buscar".into(),
                cx,
            ))
            .child(self.nav_item(
                "nav-now-playing",
                ActivePage::NowPlaying,
                "icons/play.svg",
                "Tocando agora".into(),
                cx,
            ))
            .child(self.nav_item(
                "nav-import",
                ActivePage::Import,
                "icons/import.svg",
                "Importar playlists".into(),
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(MUTED))
                            .child("SUA BIBLIOTECA"),
                    )
                    .child(
                        Button::new("sidebar-create-playlist")
                            .ghost()
                            .compact()
                            .child(svg_icon("icons/plus.svg", INFO, 12.0))
                            .child("Criar")
                            .tooltip("Criar playlist")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_page = ActivePage::Library;
                                cx.notify();
                            })),
                    ),
            )
            .child(self.nav_item(
                "nav-liked-songs",
                ActivePage::Library,
                "icons/heart-fill.svg",
                format!("Músicas curtidas ({})", self.liked_tracks.len()),
                cx,
            ))
            .child(self.nav_item(
                "nav-queue",
                ActivePage::Queue,
                "icons/queue.svg",
                format!("Fila de reprodução ({})", self.playback.queue.len()),
                cx,
            ))
            .child(if compact {
                div().flex_1().into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .px_3()
                            .pt_3()
                            .pb_1()
                            .text_xs()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(MUTED))
                            .child(format!("PLAYLISTS ({})", self.playlists.len())),
                    )
                    .child(playlist_list)
                    .into_any_element()
            })
    }

    fn mobile_nav_item(
        &self,
        id: &'static str,
        page: ActivePage,
        icon_path: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.active_page == page;
        let fg = if active { ACCENT } else { MUTED };
        Button::new(id)
            .ghost()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .py_1()
            .compact()
            .cursor_pointer()
            .text_color(rgb(fg))
            .bg(rgb(if active { SURFACE } else { BLACK }))
            .child(svg_icon(icon_path, fg, 16.0))
            .child(div().max_w_full().text_xs().truncate().child(label))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_page = page;
                cx.notify();
            }))
    }

    fn mobile_nav_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h(px(56.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px_2()
            .bg(rgb(BLACK))
            .border_t_1()
            .border_color(rgb(CARD))
            .child(self.mobile_nav_item(
                "mobile-nav-home",
                ActivePage::Home,
                "icons/home.svg",
                "Início",
                cx,
            ))
            .child(self.mobile_nav_item(
                "mobile-nav-search",
                ActivePage::Search,
                "icons/search.svg",
                "Buscar",
                cx,
            ))
            .child(self.mobile_nav_item(
                "mobile-nav-library",
                ActivePage::Library,
                "icons/heart-fill.svg",
                "Biblioteca",
                cx,
            ))
            .child(self.mobile_nav_item(
                "mobile-nav-queue",
                ActivePage::Queue,
                "icons/queue.svg",
                "Fila",
                cx,
            ))
            .child(self.mobile_nav_item(
                "mobile-nav-import",
                ActivePage::Import,
                "icons/import.svg",
                "Importar",
                cx,
            ))
    }

    fn now_playing_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.playback.title.is_empty() {
            "Nada tocando"
        } else {
            &self.playback.title
        };
        let artist = if self.playback.artist.is_empty() {
            "Pesquise uma faixa"
        } else {
            &self.playback.artist
        };
        let mark: String = title.chars().take(2).collect();
        let queue_count = self.playback.queue.len();
        let next_track = self
            .playback
            .next_index
            .and_then(|index| self.playback.queue.get(index).map(|entry| &entry.item));
        let next_track_content = if let Some(track) = next_track {
            let mark: String = track.title.chars().take(2).collect();
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(artwork(
                    &mark,
                    cover_color(&track.title),
                    40.0,
                    track.artwork_url.as_deref(),
                ))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_sm().truncate().child(track.title.clone()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(MUTED))
                                .truncate()
                                .child(track.artist.clone()),
                        ),
                )
                .into_any_element()
        } else {
            let message = if self.playback.shuffle && queue_count > 1 {
                "A próxima faixa será escolhida aleatoriamente."
            } else {
                "Nenhuma próxima faixa."
            };
            div()
                .text_xs()
                .text_color(rgb(MUTED))
                .child(message)
                .into_any_element()
        };

        div()
            .w(px(272.0))
            .flex_none()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_4()
            .p_4()
            .rounded_md()
            .bg(rgb(SURFACE))
            .child(
                div()
                    .w_full()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Tocando agora"),
            )
            .child(artwork(
                &mark,
                cover_color(title),
                200.0,
                self.playback.artwork_url.as_deref(),
            ))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .w_full()
                            .text_base()
                            .font_weight(gpui::FontWeight::BOLD)
                            .truncate()
                            .child(title.to_owned()),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(artist.to_owned()),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(CARD))
                    .child(div().text_xs().text_color(rgb(MUTED)).child("Créditos"))
                    .child(div().text_sm().truncate().child(artist.to_owned())),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(CARD))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child("Próxima na fila"),
                            )
                            .child(next_track_content)
                            .child(div().text_sm().child(format!("{queue_count} faixas"))),
                    )
                    .child(
                        Button::new("now-playing-open-queue")
                            .ghost()
                            .compact()
                            .label("Abrir fila")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.active_page = ActivePage::Queue;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn compact_player_bar(&self, mobile: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.playback.title.is_empty() {
            "Nada tocando"
        } else {
            &self.playback.title
        };
        let artist = if self.playback.artist.is_empty() {
            "Pesquise uma faixa"
        } else {
            &self.playback.artist
        };
        let mark: String = title.chars().take(2).collect();
        let mut row = div()
            .w_full()
            .h(px(if mobile { 54.0 } else { 68.0 }))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .child(artwork(
                &mark,
                cover_color(title),
                if mobile { 38.0 } else { 48.0 },
                self.playback.artwork_url.as_deref(),
            ))
            .child(
                Button::new("compact-player-details")
                    .ghost()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_1()
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.active_page = ActivePage::NowPlaying;
                        cx.notify();
                    }))
                    .child(div().w_full().text_sm().truncate().child(title.to_owned()))
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(artist.to_owned()),
                    ),
            );

        let mut controls = div().flex().items_center().justify_center().gap_3();
        if !mobile {
            controls = controls.child(self.icon_control_button(
                "icons/prev.svg",
                "compact-prev",
                PlaybackCommand::Previous,
                false,
                16.0,
                cx,
            ));
        }
        controls = controls.child(
            Button::new("compact-play-pause")
                .primary()
                .bg(rgb(0xffffff))
                .text_color(rgb(BLACK))
                .compact()
                .child(svg_icon(
                    if self.playback.playing {
                        "icons/pause.svg"
                    } else {
                        "icons/play.svg"
                    },
                    BLACK,
                    16.0,
                ))
                .on_click(
                    cx.listener(|this, _, _, cx| this.run_command(PlaybackCommand::PlayPause, cx)),
                ),
        );
        if !mobile {
            controls = controls.child(self.icon_control_button(
                "icons/next.svg",
                "compact-next",
                PlaybackCommand::Next,
                false,
                16.0,
                cx,
            ));
        }
        row = row.child(controls.flex_1());
        if !mobile {
            row = row
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(svg_icon("icons/volume.svg", INFO, 16.0))
                        .child(
                            div().w(px(80.0)).child(
                                Slider::new(&self.volume).disabled(!self.playback.connected),
                            ),
                        ),
                )
                .child(
                    div()
                        .w(px(120.0))
                        .child(Slider::new(&self.seek).disabled(self.playback.duration <= 0)),
                );
        }

        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(BLACK))
            .border_t_1()
            .border_color(rgb(CARD))
            .child(row)
            .child(div().h(px(2.0)).w_full().bg(rgb(if self.playback.playing {
                ACCENT
            } else {
                PRIMARY
            })))
    }

    fn full_player_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.playback.title.is_empty() {
            if self.playback.connected {
                "Nada tocando"
            } else {
                "Player iniciando"
            }
        } else {
            &self.playback.title
        };
        let artist = if self.playback.loading {
            "Carregando…"
        } else if self.playback.artist.is_empty() {
            if self.playback.connected {
                "Pesquise uma faixa"
            } else {
                "Aguarde o player de áudio iniciar"
            }
        } else {
            &self.playback.artist
        };
        let mark: String = title.chars().take(2).collect();
        div()
            .h(px(80.0))
            .flex_none()
            .flex()
            .flex_col()
            .justify_between()
            .bg(rgb(BLACK))
            .border_t_1()
            .border_color(rgb(CARD))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .child(
                        div()
                            .w(px(220.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(artwork(
                                &mark,
                                cover_color(title),
                                48.0,
                                self.playback.artwork_url.as_deref(),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .overflow_hidden()
                                    .text_xs()
                                    .child(div().truncate().child(title.to_owned()))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_color(rgb(MUTED))
                                            .child(artist.to_owned()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_4()
                                    .child(self.icon_control_button(
                                        "icons/shuffle.svg",
                                        "shuffle",
                                        PlaybackCommand::Shuffle,
                                        self.playback.shuffle,
                                        16.0,
                                        cx,
                                    ))
                                    .child(self.icon_control_button(
                                        "icons/prev.svg",
                                        "previous",
                                        PlaybackCommand::Previous,
                                        false,
                                        16.0,
                                        cx,
                                    ))
                                    .child(self.control_button(
                                        "−10",
                                        "seek-back",
                                        PlaybackCommand::SeekBack,
                                        false,
                                        cx,
                                    ))
                                    .child(if self.playback.retry {
                                        self.control_button(
                                            "Tentar novamente",
                                            "play-pause",
                                            PlaybackCommand::PlayPause,
                                            true,
                                            cx,
                                        )
                                        .into_any_element()
                                    } else {
                                        Button::new("play-pause")
                                            .primary()
                                            .size(px(32.0))
                                            .rounded_full()
                                            .p_0()
                                            .bg(rgb(0xffffff))
                                            .text_color(rgb(BLACK))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.run_command(PlaybackCommand::PlayPause, cx)
                                            }))
                                            .child(svg_icon(
                                                if self.playback.playing {
                                                    "icons/pause.svg"
                                                } else {
                                                    "icons/play.svg"
                                                },
                                                BLACK,
                                                16.0,
                                            ))
                                            .into_any_element()
                                    })
                                    .child(self.control_button(
                                        "+10",
                                        "seek-forward",
                                        PlaybackCommand::SeekForward,
                                        false,
                                        cx,
                                    ))
                                    .child(self.icon_control_button(
                                        "icons/next.svg",
                                        "next",
                                        PlaybackCommand::Next,
                                        false,
                                        16.0,
                                        cx,
                                    ))
                                    .child(self.icon_control_button(
                                        "icons/repeat.svg",
                                        "repeat",
                                        PlaybackCommand::Repeat,
                                        self.playback.repeat != "None",
                                        16.0,
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(playback::format_time(self.playback.position))
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Slider::new(&self.seek)
                                                .disabled(self.playback.duration <= 0),
                                        ),
                                    )
                                    .child(playback::format_time(self.playback.duration)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(200.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .text_color(rgb(MUTED))
                            .child(svg_icon("icons/volume.svg", INFO, 16.0))
                            .child(self.control_button(
                                "−",
                                "volume-down",
                                PlaybackCommand::VolumeDown,
                                false,
                                cx,
                            ))
                            .child(div().w(px(76.0)).child(
                                Slider::new(&self.volume).disabled(!self.playback.connected),
                            ))
                            .child(self.control_button(
                                "+",
                                "volume-up",
                                PlaybackCommand::VolumeUp,
                                false,
                                cx,
                            )),
                    ),
            )
            .child(div().h(px(2.0)).w_full().bg(rgb(if self.playback.playing {
                ACCENT
            } else {
                PRIMARY
            })))
    }

    fn control_button(
        &self,
        label: &'static str,
        id: &'static str,
        command: PlaybackCommand,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Button::new(id)
            .ghost()
            .compact()
            .text_color(rgb(if active { ACCENT } else { MUTED }))
            .on_click(cx.listener(move |this, _, _, cx| this.run_command(command, cx)))
            .child(label)
    }

    fn icon_control_button(
        &self,
        icon_path: &'static str,
        id: &'static str,
        command: PlaybackCommand,
        active: bool,
        size: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tooltip = match id {
            "shuffle" | "now-playing-shuffle" => "Modo aleatório",
            "previous" | "compact-prev" | "now-playing-prev" => "Faixa anterior",
            "next" | "compact-next" | "now-playing-next" => "Próxima faixa",
            "repeat" | "now-playing-repeat" => "Repetir faixa",
            _ => "Controle de reprodução",
        };
        Button::new(id)
            .ghost()
            .compact()
            .tooltip(tooltip)
            .on_click(cx.listener(move |this, _, _, cx| this.run_command(command, cx)))
            .child(svg_icon(
                icon_path,
                if active { ACCENT } else { MUTED },
                size,
            ))
    }
}

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(path) = self.pending_file_path.take() {
            self.spotify_file_input.update(cx, |input, cx| {
                input.set_value(path, window, cx);
            });
        }

        let progress = if self.playback.duration > 0 {
            (self.playback.position as f32 / self.playback.duration as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let volume = self.playback.volume.clamp(0.0, 1.0) as f32;
        let width = window.bounds().size.width;
        let mobile = width <= px(620.0);
        let compact_navigation = width <= px(900.0);
        let compact_player = width < px(1040.0);
        let show_now_playing = width >= px(1040.0);

        for (slider, value) in [(&self.seek, progress), (&self.volume, volume)] {
            slider.update(cx, |slider, cx| {
                if slider.value().start() != value * 100. {
                    slider.set_value(value * 100., window, cx);
                }
            });
        }
        let mut content = div()
            .flex()
            .flex_1()
            .min_h_0()
            .gap(px(if mobile { 0.0 } else { 4.0 }));
        if !mobile {
            content = content.child(self.sidebar(compact_navigation, cx));
        }
        content = content.child(self.content(cx));
        if show_now_playing {
            content = content.child(self.now_playing_panel(cx));
        }

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BLACK))
            .text_color(rgb(0xffffff))
            .child(header(
                &self.search_input,
                mobile,
                cx.listener(|this, _, _, cx| {
                    this.active_page = ActivePage::Home;
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| this.search(cx)),
            ))
            .child(content);
        root = if compact_player {
            root.child(self.compact_player_bar(mobile, cx))
        } else {
            root.child(self.full_player_bar(cx))
        };
        if mobile {
            root = root.child(self.mobile_nav_bar(cx));
        }
        root
    }
}

pub(crate) fn run() {
    Application::new()
        .with_assets(Assets)
        .with_http_client(Arc::new(Http))
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);
            {
                let theme = gpui_component::Theme::global_mut(cx);
                theme.background = rgb(BLACK).into();
                theme.foreground = rgb(0xffffff).into();
                theme.muted_foreground = rgb(MUTED).into();
                theme.primary = rgb(0xffffff).into();
                theme.primary_foreground = rgb(BLACK).into();
                theme.secondary = rgb(CARD).into();
                theme.secondary_hover = rgb(CARD_HOVER).into();
                theme.secondary_foreground = rgb(0xffffff).into();
                theme.accent = rgb(CARD_HOVER).into();
                theme.accent_foreground = rgb(0xffffff).into();
                theme.border = rgb(CARD_HOVER).into();
                theme.input = rgb(CARD).into();
                theme.caret = rgb(0xffffff).into();
                theme.slider_bar = rgb(0xffffff).into();
                theme.slider_thumb = rgb(0xffffff).into();
            }
            let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let home = cx.new(|cx| {
                        let home = Home::new(window, cx);
                        home.start_polling(cx);
                        home
                    });
                    cx.new(|cx| Root::new(home, window, cx))
                },
            )
            .expect("failed to open the main window");
            cx.activate(true);
        });
}
