use std::borrow::Cow;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Duration;

use crate::design::{ACCENT, BLACK, CARD, CARD_HOVER, ERROR, INFO, MUTED, PRIMARY};
use gpui::http_client::http::HeaderValue;
use gpui::http_client::{
    AsyncBody, HttpClient, Request, Response, Result as HttpResult, Url, anyhow,
};
use gpui::{
    AnyElement, App, AssetSource, ClickEvent, Entity, ImageSource, ObjectFit, SharedString,
    SharedUri, Svg, Window, div, img, prelude::*, px, rgb, svg,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    input::{Input, InputState},
};

const ICON_PATHS: &[&str] = &[
    "icons/home.svg",
    "icons/search.svg",
    "icons/heart.svg",
    "icons/heart-fill.svg",
    "icons/play.svg",
    "icons/pause.svg",
    "icons/prev.svg",
    "icons/next.svg",
    "icons/shuffle.svg",
    "icons/repeat.svg",
    "icons/queue.svg",
    "icons/library.svg",
    "icons/volume.svg",
    "icons/external.svg",
    "icons/plus.svg",
    "icons/music.svg",
];

/// Fonte de assets embutidos para renderização de ícones SVG via `gpui::svg`.
pub(crate) struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "icons/home.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V20a1 1 0 0 0 1 1h4v-6h4v6h4a1 1 0 0 0 1-1V9.5"/></svg>"#),
            "icons/search.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"#),
            "icons/heart.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20.5s-7-4.6-9.2-8.7C1 8.5 2.8 4.5 6.8 4.5c2.1 0 3.9 1.1 5.2 2.8 1.3-1.7 3.1-2.8 5.2-2.8 4 0 5.8 4 4 7.3-2.2 4.1-9.2 8.7-9.2 8.7Z"/></svg>"#),
            "icons/heart-fill.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="black"><path d="M12 21s-7.4-4.8-9.7-9.1C.3 8.2 2.3 4 6.6 4c2.2 0 4.1 1.1 5.4 2.9C13.3 5.1 15.2 4 17.4 4c4.3 0 6.3 4.2 4.3 7.9C19.4 16.2 12 21 12 21Z"/></svg>"#),
            "icons/play.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="black"><path d="M7 4.5v15l12-7.5-12-7.5Z"/></svg>"#),
            "icons/pause.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="black"><rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/></svg>"#),
            "icons/prev.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="black"><rect x="5" y="5" width="2.5" height="14" rx="1"/><path d="M19 5.5v13L9.5 12 19 5.5Z"/></svg>"#),
            "icons/next.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="black"><path d="M5 5.5v13l9.5-6.5L5 5.5Z"/><rect x="16.5" y="5" width="2.5" height="14" rx="1"/></svg>"#),
            "icons/shuffle.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M16 3h5v5"/><path d="M4 20 21 3"/><path d="M21 16v5h-5"/><path d="m15 15 6 6"/><path d="m4 4 5 5"/></svg>"#),
            "icons/repeat.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m17 2 4 4-4 4"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/></svg>"#),
            "icons/queue.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h16"/><path d="M4 12h16"/><path d="M4 18h10"/></svg>"#),
            "icons/library.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 19V5"/><path d="M9 19V5"/><path d="m14 5 5 14"/></svg>"#),
            "icons/volume.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H3v6h3l5 4V5Z"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M18.5 5.5a9 9 0 0 1 0 13"/></svg>"#),
            "icons/external.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6"/><path d="M10 14 20 4"/><path d="M20 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h5"/></svg>"#),
            "icons/plus.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14"/><path d="M5 12h14"/></svg>"#),
            "icons/music.svg" => Some(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>"#),
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        if path.is_empty() || path == "icons" {
            Ok(ICON_PATHS.iter().copied().map(SharedString::from).collect())
        } else {
            Ok(Vec::new())
        }
    }
}

/// Cliente HTTP baseado em `ureq` para carregamento assíncrono de capas no GPUI.
pub(crate) struct Http;

fn image_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build();
        ureq::Agent::new_with_config(config)
    })
}

impl HttpClient for Http {
    fn type_name(&self) -> &'static str {
        "rea_spotify::components::Http"
    }

    fn user_agent(&self) -> Option<&HeaderValue> {
        None
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(
        &self,
        req: Request<AsyncBody>,
    ) -> Pin<Box<dyn Future<Output = HttpResult<Response<AsyncBody>>> + Send + 'static>> {
        let uri = req.uri().to_string();
        Box::pin(async move {
            let mut response = image_agent()
                .get(&uri)
                .call()
                .map_err(|err| anyhow!("failed to fetch {uri}: {err}"))?;
            let status = response.status().as_u16();
            let bytes = response
                .body_mut()
                .read_to_vec()
                .map_err(|err| anyhow!("failed to read {uri}: {err}"))?;
            Response::builder()
                .status(status)
                .body(AsyncBody::from(bytes))
                .map_err(|err| anyhow!("failed to build response for {uri}: {err}"))
        })
    }
}

/// Renderiza um ícone SVG registrado em `Assets` com cor e tamanho definidos.
pub(crate) fn svg_icon(path: &'static str, color: u32, size: f32) -> Svg {
    svg()
        .path(path)
        .size(px(size))
        .flex_none()
        .text_color(rgb(color))
}

/// Ícone de favorito estilizado para faixas curtidas ou não curtidas.
pub(crate) fn icon_heart(liked: bool) -> Svg {
    svg_icon(
        if liked {
            "icons/heart-fill.svg"
        } else {
            "icons/heart.svg"
        },
        if liked { ACCENT } else { MUTED },
        16.0,
    )
}

pub(crate) fn header(
    search: &Entity<InputState>,
    connected: bool,
    on_home: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_search: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let status_color = if connected { ACCENT } else { INFO };
    div()
        .h(px(48.0))
        .flex()
        .items_center()
        .gap_4()
        .px_4()
        .bg(rgb(BLACK))
        .border_b_1()
        .border_color(rgb(CARD))
        .text_color(rgb(MUTED))
        .child(div().flex_1())
        .child(
            Button::new("home")
                .ghost()
                .tooltip("Início")
                .child(svg_icon("icons/home.svg", 0xffffff, 18.0))
                .on_click(on_home),
        )
        .child(
            div()
                .w(px(500.0))
                .h(px(34.0))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .rounded_full()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgb(CARD_HOVER))
                .text_sm()
                .child(svg_icon("icons/search.svg", PRIMARY, 16.0))
                .child(Input::new(search).appearance(false).w_full())
                .child(
                    Button::new("search-submit")
                        .ghost()
                        .compact()
                        .tooltip("Pesquisar")
                        .child(svg_icon("icons/search.svg", MUTED, 14.0))
                        .on_click(on_search),
                ),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .rounded_full()
                .bg(rgb(CARD))
                .border_1()
                .border_color(rgb(status_color))
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(status_color))
                .child(div().size(px(8.0)).rounded_full().bg(rgb(status_color)))
                .child(if connected {
                    "YouTube · Audius"
                } else {
                    "Player iniciando"
                }),
        )
}

fn artwork_placeholder(mark: String, color: u32, side: f32) -> AnyElement {
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
        .child(mark)
        .into_any_element()
}

/// Exibe a capa real (URL ou caminho local) com fallback colorido quando ausente ou indisponível.
pub(crate) fn artwork(mark: &str, color: u32, side: f32, image_path: Option<&str>) -> AnyElement {
    let Some(path) = image_path.map(str::trim).filter(|path| !path.is_empty()) else {
        return artwork_placeholder(mark.to_owned(), color, side);
    };
    let source: ImageSource = if path.starts_with("http://") || path.starts_with("https://") {
        SharedUri::from(path.to_owned()).into()
    } else {
        PathBuf::from(path).into()
    };
    let fallback_mark = mark.to_owned();
    let loading_mark = mark.to_owned();
    div()
        .size(px(side))
        .flex_none()
        .rounded_sm()
        .overflow_hidden()
        .bg(rgb(color))
        .child(
            img(source)
                .size(px(side))
                .flex_none()
                .overflow_hidden()
                .rounded_sm()
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || artwork_placeholder(fallback_mark.clone(), color, side))
                .with_loading(move || artwork_placeholder(loading_mark.clone(), color, side)),
        )
        .into_any_element()
}

/// Cartão informativo com destaque cromático conforme a severidade (`error`, `info`, `success`).
pub(crate) fn status_card(title: &str, message: &str, kind: &str) -> impl IntoElement {
    let accent = match kind {
        "error" => ERROR,
        "info" => INFO,
        "success" => ACCENT,
        _ => PRIMARY,
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .rounded_md()
        .bg(rgb(CARD))
        .border_l_4()
        .border_color(rgb(accent))
        .p_6()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().size(px(8.0)).rounded_full().bg(rgb(accent)))
                .child(
                    div()
                        .text_lg()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgb(0xffffff))
                        .child(title.to_owned()),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(MUTED))
                .child(message.to_owned()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_svg_icons_are_listed_and_valid() {
        let listed = Assets.list("icons").unwrap();
        assert_eq!(listed.len(), ICON_PATHS.len());
        for path in ICON_PATHS {
            let bytes = Assets.load(path).unwrap().expect("missing icon asset");
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.starts_with("<svg ") && text.ends_with("</svg>"));
        }
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
    }
}
