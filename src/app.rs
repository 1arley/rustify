use crate::components::{artwork, header, status_card};
use crate::design::{BLACK, CARD, MUTED, SURFACE};
use crate::home::playback_summary;
use crate::playback::{self, PlaybackCommand, PlaybackState};
use crate::spotify::{self, Track};
use gpui::{
    App, Application, Bounds, Context, Render, Subscription, Timer, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
};
use std::time::Duration;

struct Home {
    playback: PlaybackState,
    client_id: String,
    search_input: gpui::Entity<InputState>,
    client_id_input: gpui::Entity<InputState>,
    _search_subscription: Subscription,
    results: Vec<Track>,
    searching: bool,
    connecting: bool,
    connected_account: bool,
    message: Option<String>,
}

impl Home {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let client_id = spotify::client_id().unwrap_or_default();
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("O que você quer ouvir?"));
        let client_id_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Client ID do Spotify Developer Dashboard")
                .default_value(client_id.clone())
        });
        let _search_subscription = cx.subscribe(&search_input, |this, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.search(cx);
            }
        });

        Self {
            playback: PlaybackState::default(),
            client_id,
            search_input,
            client_id_input,
            _search_subscription,
            results: Vec::new(),
            searching: false,
            connecting: false,
            connected_account: false,
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

    fn save_client_id(&mut self, cx: &mut Context<Self>) {
        let value = self.client_id_input.read(cx).value().to_string();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::save_client_id(&value) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(client_id) => {
                        view.client_id = client_id;
                        view.connected_account = false;
                        view.message =
                            Some("Client ID salvo. Agora conecte sua conta Spotify.".to_owned());
                    }
                    Err(error) => view.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn connect_account(&mut self, cx: &mut Context<Self>) {
        if self.connecting {
            return;
        }
        if self.client_id.is_empty() {
            self.message =
                Some("Configure primeiro o Client ID do Spotify Developer Dashboard.".to_owned());
            cx.notify();
            return;
        }

        let client_id = self.client_id.clone();
        self.connecting = true;
        self.message = Some("Aguardando autorização do Spotify no navegador…".to_owned());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::connect(&client_id) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.connecting = false;
                match result {
                    Ok(()) => {
                        view.connected_account = true;
                        view.message =
                            Some("Conta Spotify conectada. Pesquise uma faixa acima.".to_owned());
                    }
                    Err(error) => view.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        if self.searching {
            return;
        }
        let query = self.search_input.read(cx).value().trim().to_owned();
        if query.is_empty() {
            return;
        }
        if self.client_id.is_empty() {
            self.message = Some("Configure o Client ID do Spotify antes de pesquisar.".to_owned());
            cx.notify();
            return;
        }

        let client_id = self.client_id.clone();
        self.searching = true;
        self.results.clear();
        self.message = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { spotify::search_tracks(&client_id, &query) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.searching = false;
                match result {
                    Ok(results) => {
                        view.connected_account = true;
                        view.results = results;
                        if view.results.is_empty() {
                            view.message = Some("Nenhuma faixa encontrada.".to_owned());
                        }
                    }
                    Err(error) => view.message = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn play_track(&mut self, uri: String, cx: &mut Context<Self>) {
        self.message = Some("Abrindo faixa no Spotify Desktop…".to_owned());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { playback::play_track(&uri) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.message = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    fn content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let client_field = Input::new(&self.client_id_input).w_full();
        let rows = self
            .results
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, track)| {
                let uri = track.uri.clone();
                let details = format!("{} · {}", track.artist_names(), track.album_name());
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
                        Button::new(("track-play", index))
                            .ghost()
                            .compact()
                            .label("▶")
                            .tooltip("Tocar faixa")
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.play_track(uri.clone(), cx)),
                            ),
                    )
            });

        div()
            .flex_1()
            .min_h_0()
            .id("home-content")
            .overflow_scroll()
            .rounded_md()
            .bg(rgb(SURFACE))
            .child(
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
                            .child(div().text_xs().text_color(rgb(MUTED)).child("SPOTIFY REAL"))
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(if self.connected_account {
                                        "Pesquise e ouça música"
                                    } else {
                                        "Conecte sua conta Spotify"
                                    }),
                            )
                            .child(if self.client_id.is_empty() {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(div().text_sm().text_color(rgb(MUTED)).child(
                                        "Crie um app no Spotify Developer Dashboard, cadastre http://127.0.0.1/callback como Redirect URI e cole o Client ID abaixo.",
                                    ))
                                    .child(div().text_xs().child("Client ID do Spotify"))
                                    .child(client_field)
                                    .child(self.action_button(
                                        "Salvar Client ID",
                                        "save-client-id",
                                        cx.listener(|this, _, _, cx| this.save_client_id(cx)),
                                    ))
                            } else {
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_4()
                                    .child(div().text_sm().text_color(rgb(MUTED)).child(if self.connected_account {
                                        "Busca pelo catálogo oficial; o áudio toca no Spotify Desktop instalado."
                                    } else {
                                        "A autorização usa OAuth PKCE e guarda tokens no chaveiro do sistema."
                                    }))
                                    .child(self.action_button(
                                        if self.connecting {
                                            "Conectando…"
                                        } else if self.connected_account {
                                            "Verificar conta"
                                        } else {
                                            "Conectar Spotify"
                                        },
                                        "connect-spotify",
                                        cx.listener(|this, _, _, cx| this.connect_account(cx)),
                                    ))
                            })
                            .child(if self.searching {
                                div().text_sm().text_color(rgb(MUTED)).child("Pesquisando no catálogo Spotify…")
                            } else {
                                div().children(rows)
                            })
                            .child(if let Some(message) = &self.message {
                                div()
                                    .rounded_md()
                                    .bg(rgb(0x292929))
                                    .p_3()
                                    .text_sm()
                                    .child(message.clone())
                            } else {
                                div().child("")
                            }),
                    ),
            )
    }

    fn action_button(
        &self,
        label: &'static str,
        id: &'static str,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        Button::new(id).primary().label(label).on_click(on_click)
    }

    fn sidebar(&self) -> impl IntoElement {
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
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_sm()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Sua Biblioteca"),
            )
            .child(status_card(
                "Biblioteca",
                "Os itens da sua conta aparecerão aqui quando a Web API for configurada.",
            ))
    }

    fn player_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.playback.title.is_empty() {
            if self.playback.connected {
                "Nada tocando"
            } else {
                "Spotify Desktop desconectado"
            }
        } else {
            &self.playback.title
        };
        let artist = if self.playback.artist.is_empty() {
            if self.playback.connected {
                "Escolha uma música no Spotify"
            } else {
                "Abra o Spotify e escolha uma música"
            }
        } else {
            &self.playback.artist
        };
        let mark: String = title.chars().take(2).collect();
        let progress = if self.playback.duration > 0 {
            (self.playback.position as f32 / self.playback.duration as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let volume = self.playback.volume.clamp(0.0, 1.0) as f32;

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
                                        if self.playback.playing {
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
                                        div()
                                            .w(px(360.0))
                                            .h(px(4.0))
                                            .rounded_full()
                                            .bg(rgb(0x4b4b4b))
                                            .child(
                                                div()
                                                    .w(px(360.0 * progress))
                                                    .h_full()
                                                    .rounded_full()
                                                    .bg(rgb(0x1ed760)),
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
                            .child(
                                div()
                                    .w(px(76.0))
                                    .h(px(4.0))
                                    .rounded_full()
                                    .bg(rgb(0x4b4b4b))
                                    .child(
                                        div()
                                            .w(px(76.0 * volume))
                                            .h_full()
                                            .rounded_full()
                                            .bg(rgb(0xffffff)),
                                    ),
                            )
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BLACK))
            .text_color(rgb(0xffffff))
            .child(header(
                &self.search_input,
                self.playback.connected,
                cx.listener(|this, _, _, cx| this.search(cx)),
            ))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(4.0))
                    .child(self.sidebar())
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
