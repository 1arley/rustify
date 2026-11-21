use crate::design::{MUTED, SURFACE};
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
        "Pesquise no catálogo Audius para começar a ouvir"
    } else {
        &state.artist
    };

    div().w_full().rounded_md().bg(rgb(SURFACE)).p_4().child(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(MUTED))
                    .child("TOCANDO AGORA"),
            )
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(title.to_owned()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(MUTED))
                    .child(artist.to_owned()),
            ),
    )
}
