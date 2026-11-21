use crate::design::{BLACK, CARD, MUTED};
use gpui::{App, ClickEvent, Entity, Window, div, prelude::*, px, rgb};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};

pub(crate) fn header(
    search: &Entity<InputState>,
    connected: bool,
    on_home: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_search: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .h(px(40.0))
        .flex()
        .items_center()
        .gap_4()
        .px_4()
        .bg(rgb(BLACK))
        .text_color(rgb(MUTED))
        .child(div().flex_1())
        .child(
            Button::new("home")
                .ghost()
                .label("⌂")
                .tooltip("Início")
                .on_click(on_home),
        )
        .child(
            div()
                .w(px(500.0))
                .h(px(32.0))
                .flex()
                .items_center()
                .px_3()
                .rounded_full()
                .bg(rgb(0x242424))
                .text_sm()
                .child("⌕")
                .child(Input::new(search).appearance(false).w_full())
                .child(
                    Button::new("search-submit")
                        .ghost()
                        .compact()
                        .label("↵")
                        .tooltip("Pesquisar")
                        .on_click(on_search),
                ),
        )
        .child(div().flex_1())
        .child(div().text_sm().child(if connected {
            "YouTube · Audius"
        } else {
            "Player iniciando"
        }))
}

pub(crate) fn artwork(mark: &str, color: u32, side: f32) -> impl IntoElement {
    div()
        .size(px(side))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .bg(rgb(color))
        .text_color(rgb(0xffffff))
        .text_sm()
        .font_weight(gpui::FontWeight::BOLD)
        .child(mark.to_owned())
}

pub(crate) fn status_card(title: &str, message: &str) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .rounded_md()
        .bg(rgb(CARD))
        .p_6()
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
                .child(message.to_owned()),
        )
}
