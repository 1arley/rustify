use crate::catalog::{Source, Track};
use crate::components::{artwork, header, status_card};
use crate::design::{BLACK, CARD, MUTED, SURFACE};
use crate::home::playback_summary;
use crate::library;
use crate::playback::{self, PlaybackCommand, PlaybackItem, PlaybackState, QueueCommand};
use gpui::{
    AnyElement, App, Application, Bounds, Context, Render, Subscription, Timer, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::slider::{Slider, SliderEvent, SliderState};
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
};
use std::time::Duration;

fn playback_item(track: &Track) -> PlaybackItem {
    PlaybackItem {
        track_id: track.id.clone(),
        title: track.name.clone(),
        artist: track.artist_names().to_owned(),
        duration: track.duration_micros(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActivePage {
    Home,
    Search,
    Library,
    Queue,
}

struct Home {
    seek: gpui::Entity<SliderState>,
    volume: gpui::Entity<SliderState>,
    playback: PlaybackState,
    search_input: gpui::Entity<InputState>,
    _search_subscription: Subscription,
    results: Vec<Track>,
    active_page: ActivePage,
    liked_tracks: Vec<Track>,
    library_loaded: bool,
    library_error: Option<String>,
    searching: bool,
    message: Option<String>,
}

impl Home {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (liked_tracks, library_loaded, library_error) = match library::load_liked_tracks() {
            Ok(tracks) => (tracks, true, None),
            Err(error) => (Vec::new(), false, Some(error)),
        };
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("O que você quer ouvir?"));
        let _search_subscription = cx.subscribe(&search_input, |this, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.search(Source::Youtube, cx);
            }
        });

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
            _search_subscription,
            results: Vec::new(),
            active_page: ActivePage::Home,
            liked_tracks,
            library_loaded,
            library_error,
            searching: false,
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

    fn search(&mut self, source: Source, cx: &mut Context<Self>) {
        self.active_page = ActivePage::Search;
        if self.searching {
            return;
        }
        let query = self.search_input.read(cx).value().trim().to_owned();
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
                .spawn(async move { source.search(&query) })
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

    fn toggle_liked(&mut self, track: Track, cx: &mut Context<Self>) {
        if self.liked_tracks.iter().any(|liked| liked.id == track.id) {
            self.liked_tracks.retain(|liked| liked.id != track.id);
        } else {
            self.liked_tracks.push(track);
        }
        if self.library_loaded {
            match library::save_liked_tracks(&self.liked_tracks) {
                Ok(()) => self.library_error = None,
                Err(error) => {
                    self.library_error = Some(error.clone());
                    self.message = Some(error);
                }
            }
        }
        cx.notify();
    }

    fn content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = match self.active_page {
            ActivePage::Home => div()
                .p_6()
                .flex()
                .flex_col()
                .gap_4()
                .child(div().text_2xl().child("Início"))
                .child(playback_summary(&self.playback))
                .child(div().child("Músicas curtidas"))
                .child(self.track_rows(&self.liked_tracks, cx))
                .child(self.message_card())
                .into_any_element(),
            ActivePage::Search => self.search_panel(cx),
            ActivePage::Library => self.library_panel(cx),
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

    fn search_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.track_rows(&self.results, cx);
        let content = if self.searching {
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child("Pesquisando faixas…")
                .into_any_element()
        } else if self.results.is_empty() && self.message.is_none() {
            status_card(
                "Encontre sua próxima música",
                "Pesquise faixas de artistas independentes e toque o áudio aqui no app.",
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
            .py_8()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(playback_summary(&self.playback))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .child("YOUTUBE E AUDIUS"),
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
                        Button::new("audius-search")
                            .label("Pesquisar no Audius")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.search(Source::Audius, cx)),
                            ),
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
            )
            .into_any_element()
        } else {
            self.track_rows(&self.liked_tracks, cx).into_any_element()
        };

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_6()
            .py_8()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(playback_summary(&self.playback))
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
                            .child("Salvas neste dispositivo para as próximas sessões."),
                    )
                    .child(content)
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
            )
            .into_any_element()
        } else {
            let rows = queue.iter().enumerate().map(|(index, item)| {
                let id = item.id;

                let next = queue.get(index + 1).map(|e| e.id);
                let item = &item.item;
                let is_current = current_index == Some(index);
                let can_edit = current_index.map_or(true, |current| index > current);
                let first_upcoming = current_index.map_or(0, |current| current + 1);
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
                    .min_h(px(60.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .rounded_md()
                    .bg(rgb(if is_current { 0x29362d } else { CARD }))
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
                                    .text_sm()
                                    .text_color(rgb(if is_current { 0x1ed760 } else { MUTED }))
                                    .child(if is_current {
                                        "♫".to_owned()
                                    } else {
                                        (index + 1).to_string()
                                    }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .overflow_hidden()
                                    .child(div().text_sm().child(item.title.clone()))
                                    .child(div().text_xs().text_color(rgb(MUTED)).child(
                                        if is_current {
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
                                        } else {
                                            item.artist.clone()
                                        },
                                    )),
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
                                    .compact()
                                    .label("▶")
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
                .gap_1()
                .children(rows)
                .into_any_element()
        };

        div()
            .w_full()
            .flex()
            .justify_center()
            .px_6()
            .py_8()
            .child(
                div()
                    .w_full()
                    .max_w(px(1000.0))
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(playback_summary(&self.playback))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(format!("Fila · {} faixas", queue.len())),
                    )
                    .child(content)
                    .child(self.message_card()),
            )
            .into_any_element()
    }

    fn track_rows(&self, tracks: &[Track], cx: &mut Context<Self>) -> gpui::Div {
        let queue: std::rc::Rc<[Track]> = tracks.into();
        let rows = tracks.iter().cloned().enumerate().map(|(index, track)| {
            let is_liked = self.liked_tracks.iter().any(|liked| liked.id == track.id);
            let favorite = track.clone();
            let queue_item = playback_item(&track);
            let permalink = track.permalink();
            let playback_queue = queue.clone();
            let genre = if track.album_name().is_empty() {
                "Audius"
            } else {
                track.album_name()
            };
            let details = format!("{} · {genre}", track.artist_names());
            div()
                .id(("track-row", index))
                .w_full()
                .h(px(64.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .px_4()
                .rounded_md()
                .bg(rgb(CARD))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .overflow_hidden()
                        .child(div().text_sm().child(track.name))
                        .child(div().text_xs().text_color(rgb(MUTED)).child(details)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new(("track-like", index))
                                .ghost()
                                .compact()
                                .label(if is_liked { "♥" } else { "♡" })
                                .tooltip("Salvar na biblioteca local")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_liked(favorite.clone(), cx)
                                })),
                        )
                        .child(
                            Button::new(("track-source", index))
                                .ghost()
                                .compact()
                                .label("Origem ↗")
                                .tooltip("Abrir página da faixa")
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&permalink))),
                        )
                        .child(
                            Button::new(("track-queue", index))
                                .ghost()
                                .compact()
                                .label("+")
                                .tooltip("Adicionar à fila")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.queue_action(QueueCommand::Enqueue(queue_item.clone()), cx)
                                })),
                        )
                        .child(
                            Button::new(("track-play", index))
                                .primary()
                                .compact()
                                .label("▶")
                                .tooltip("Tocar no app")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.play_track(&playback_queue, index, cx)
                                })),
                        ),
                )
        });
        div().flex().flex_col().gap_1().children(rows)
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
                .bg(rgb(0x292929))
                .p_3()
                .text_sm()
                .child(message.clone())
                .into_any_element()
        } else {
            div().into_any_element()
        }
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(272.0))
            .flex_none()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded_md()
            .bg(rgb(SURFACE))
            .child(
                div()
                    .id("nav-search")
                    .w_full()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(if self.active_page == ActivePage::Search {
                        CARD
                    } else {
                        SURFACE
                    }))
                    .text_sm()
                    .text_color(rgb(if self.active_page == ActivePage::Search {
                        0xffffff
                    } else {
                        MUTED
                    }))
                    .child("Buscar")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.active_page = ActivePage::Search;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Sua Biblioteca"),
            )
            .child(
                div()
                    .id("nav-liked-songs")
                    .w_full()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(if self.active_page == ActivePage::Library {
                        CARD
                    } else {
                        SURFACE
                    }))
                    .text_sm()
                    .text_color(rgb(if self.active_page == ActivePage::Library {
                        0xffffff
                    } else {
                        MUTED
                    }))
                    .child(format!("♡ Músicas curtidas ({})", self.liked_tracks.len()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.active_page = ActivePage::Library;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("nav-queue")
                    .w_full()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(if self.active_page == ActivePage::Queue {
                        CARD
                    } else {
                        SURFACE
                    }))
                    .text_sm()
                    .text_color(rgb(if self.active_page == ActivePage::Queue {
                        0xffffff
                    } else {
                        MUTED
                    }))
                    .child(format!(
                        "☷ Fila de reprodução ({})",
                        self.playback.queue.len()
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.active_page = ActivePage::Queue;
                        cx.notify();
                    })),
            )
    }

    fn player_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .h(px(76.0))
            .flex_none()
            .flex()
            .flex_col()
            .justify_between()
            .bg(rgb(BLACK))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .child(
                        div()
                            .w(px(260.0))
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(artwork(&mark, 0x3d5360, 44.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .text_xs()
                                    .child(title.to_owned())
                                    .child(div().text_color(rgb(MUTED)).child(artist.to_owned())),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_4()
                                    .child(self.control_button(
                                        "⤨",
                                        "shuffle",
                                        PlaybackCommand::Shuffle,
                                        self.playback.shuffle,
                                        cx,
                                    ))
                                    .child(self.control_button(
                                        "◀",
                                        "previous",
                                        PlaybackCommand::Previous,
                                        false,
                                        cx,
                                    ))
                                    .child(self.control_button(
                                        "−10",
                                        "seek-back",
                                        PlaybackCommand::SeekBack,
                                        false,
                                        cx,
                                    ))
                                    .child(self.control_button(
                                        if self.playback.retry {
                                            "Tentar novamente"
                                        } else if self.playback.playing {
                                            "❚❚"
                                        } else {
                                            "▶"
                                        },
                                        "play-pause",
                                        PlaybackCommand::PlayPause,
                                        false,
                                        cx,
                                    ))
                                    .child(self.control_button(
                                        "+10",
                                        "seek-forward",
                                        PlaybackCommand::SeekForward,
                                        false,
                                        cx,
                                    ))
                                    .child(self.control_button(
                                        "▶",
                                        "next",
                                        PlaybackCommand::Next,
                                        false,
                                        cx,
                                    ))
                                    .child(self.control_button(
                                        "↻",
                                        "repeat",
                                        PlaybackCommand::Repeat,
                                        self.playback.repeat != "None",
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(rgb(MUTED))
                                    .child(playback::format_time(self.playback.position))
                                    .child(
                                        div().w(px(360.0)).child(
                                            Slider::new(&self.seek)
                                                .disabled(self.playback.duration <= 0),
                                        ),
                                    )
                                    .child(playback::format_time(self.playback.duration)),
                            ),
                    )
                    .child(
                        div()
                            .w(px(260.0))
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .text_color(rgb(MUTED))
                            .child("◖")
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
            .child(div().h(px(2.0)).w_full().bg(rgb(CARD)))
    }

    fn control_button(
        &self,
        label: &'static str,
        id: &'static str,
        command: PlaybackCommand,
        active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .cursor_pointer()
            .text_color(rgb(if active { 0x1ed760 } else { 0xaaaaaa }))
            .on_click(cx.listener(move |this, _, _, cx| this.run_command(command, cx)))
            .child(label)
    }
}

impl Render for Home {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let progress = if self.playback.duration > 0 {
            (self.playback.position as f32 / self.playback.duration as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let volume = self.playback.volume.clamp(0.0, 1.0) as f32;

        for (slider, value) in [(&self.seek, progress), (&self.volume, volume)] {
            slider.update(cx, |slider, cx| {
                if slider.value().start() != value * 100. {
                    slider.set_value(value * 100., window, cx);
                }
            });
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BLACK))
            .text_color(rgb(0xffffff))
            .child(header(
                &self.search_input,
                self.playback.connected,
                cx.listener(|this, _, _, cx| {
                    this.active_page = ActivePage::Home;
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| this.search(Source::Youtube, cx)),
            ))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(4.0))
                    .child(self.sidebar(cx))
                    .child(self.content(cx)),
            )
            .child(self.player_bar(cx))
    }
}

pub(crate) fn run() {
    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);
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
