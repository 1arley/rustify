use std::sync::Arc;
use std::time::Duration;

use crate::catalog::{Source, Track};
use crate::components::{Assets, Http, artwork, header, icon_heart, status_card, svg_icon};
use crate::design::{
    ACCENT, BLACK, CARD, CARD_HOVER, ERROR, INFO, MUTED, PRIMARY, SURFACE, cover_color,
};
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
                .child(
                    div()
                        .text_2xl()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("Início"),
                )
                .child(playback_summary(&self.playback))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(MUTED))
                        .child("Músicas curtidas"),
                )
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
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(INFO))
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
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                Button::new("youtube-search")
                                    .primary()
                                    .bg(rgb(PRIMARY))
                                    .text_color(rgb(0xffffff))
                                    .child(svg_icon("icons/search.svg", 0xffffff, 14.0))
                                    .child("Pesquisar no YouTube")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.search(Source::Youtube, cx)
                                    })),
                            )
                            .child(
                                Button::new("audius-search")
                                    .bg(rgb(CARD))
                                    .border_color(rgb(CARD_HOVER))
                                    .text_color(rgb(0xffffff))
                                    .child(svg_icon("icons/search.svg", INFO, 14.0))
                                    .child("Pesquisar no Audius")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.search(Source::Audius, cx)
                                    })),
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
                "info",
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
            let is_liked = self.liked_tracks.iter().any(|liked| liked.id == track.id);
            let favorite = track.clone();
            let queue_item = playback_item(&track);
            let permalink = track.permalink();
            let artwork_url = track.artwork_url();
            let playback_queue = queue.clone();
            let genre = if track.album_name().is_empty() {
                "Audius"
            } else {
                track.album_name()
            };
            let details = format!("{} · {genre}", track.artist_names());
            let mark: String = if track.name.is_empty() {
                "♫".to_owned()
            } else {
                track.name.chars().take(2).collect()
            };
            let cover_bg = cover_color(&track.name);
            div()
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
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new(("track-like", index))
                                .ghost()
                                .compact()
                                .child(icon_heart(is_liked))
                                .tooltip("Salvar na biblioteca local")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_liked(favorite.clone(), cx)
                                })),
                        )
                        .child(
                            Button::new(("track-source", index))
                                .ghost()
                                .compact()
                                .text_color(rgb(MUTED))
                                .child(svg_icon("icons/external.svg", MUTED, 14.0))
                                .child(div().text_color(rgb(MUTED)).child("Origem"))
                                .tooltip("Abrir página da faixa")
                                .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&permalink))),
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
                                        this.play_items(
                                            vec![playback_item(&playback_queue[index])],
                                            0,
                                            cx,
                                        );
                                    } else {
                                        this.play_track(&playback_queue, index, cx);
                                    }
                                })),
                        ),
                )
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
        div()
            .id(id)
            .w_full()
            .flex()
            .items_center()
            .gap_3()
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
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_page = page;
                cx.notify();
            }))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(272.0))
            .flex_none()
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
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(MUTED))
                    .child("SUA BIBLIOTECA"),
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
                            .w(px(300.0))
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
                                        div()
                                            .id("play-pause")
                                            .size(px(32.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_full()
                                            .cursor_pointer()
                                            .bg(rgb(if self.playback.playing {
                                                ACCENT
                                            } else {
                                                PRIMARY
                                            }))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.run_command(PlaybackCommand::PlayPause, cx)
                                            }))
                                            .child(svg_icon(
                                                if self.playback.playing {
                                                    "icons/pause.svg"
                                                } else {
                                                    "icons/play.svg"
                                                },
                                                if self.playback.playing {
                                                    BLACK
                                                } else {
                                                    0xffffff
                                                },
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
                            .w(px(300.0))
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
        div()
            .id(id)
            .cursor_pointer()
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
        div()
            .id(id)
            .cursor_pointer()
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
                theme.primary = rgb(PRIMARY).into();
                theme.primary_foreground = rgb(0xffffff).into();
                theme.secondary = rgb(CARD).into();
                theme.secondary_hover = rgb(CARD_HOVER).into();
                theme.secondary_foreground = rgb(0xffffff).into();
                theme.accent = rgb(CARD_HOVER).into();
                theme.accent_foreground = rgb(0xffffff).into();
                theme.border = rgb(CARD_HOVER).into();
                theme.input = rgb(CARD).into();
                theme.caret = rgb(PRIMARY).into();
                theme.slider_bar = rgb(PRIMARY).into();
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
