use crate::components::artwork;
use crate::design::{ACCENT, CARD, CARD_HOVER, ERROR, INFO, MUTED, PRIMARY, cover_color};
use crate::playback::PlaybackState;
use gpui::{div, prelude::*, rgb};

pub(crate) fn playback_summary(state: &PlaybackState) -> impl IntoElement {
    let title = if state.title.is_empty() {
        if state.connected {
            "Nada tocando"
        } else {
            "Player de áudio indisponível"
        }
    } else {
        &state.title
    };
    let artist = if state.artist.is_empty() {
        "Pesquise por uma faixa para começar a ouvir"
    } else {
        &state.artist
    };
    let (status_label, status_color) = if state.loading {
        ("CARREGANDO", INFO)
    } else if state.retry {
        ("FALHA NA REPRODUÇÃO", ERROR)
    } else if state.playing {
        ("TOCANDO AGORA", ACCENT)
    } else {
        ("PLAYER", PRIMARY)
    };
    let mark: String = title.chars().take(2).collect();

    div()
        .w_full()
        .rounded_md()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgb(CARD_HOVER))
        .p_4()
        .flex()
        .items_center()
        .gap_4()
        .child(artwork(
            &mark,
            cover_color(title),
            56.0,
            state.artwork_url.as_deref(),
        ))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .flex_col()
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
                        .text_lg()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(0xffffff))
                        .truncate()
                        .child(title.to_owned()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .truncate()
                        .child(artist.to_owned()),
                ),
        )
}
