//! UI interaction test harness for GPUI.
//!
//! Uses GPUI's `#[gpui::test]` with `TestAppContext`/`VisualTestContext` to
//! exercise real component wiring and mouse interactions.
//!
//! Run:
//! - `cargo test ui_tests`
//! - `cargo test slider_click_emits_change -- --nocapture`

use gpui::{
    AppContext as _, Context, Entity, IntoElement, Modifiers, MouseButton, ParentElement, Render,
    Styled, TestAppContext, VisualTestContext, Window, div, point, px,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};

struct SliderHarness {
    slider: Entity<SliderState>,
    changes: Vec<f32>,
}

impl SliderHarness {
    fn new(cx: &mut Context<Self>) -> Self {
        let slider = cx.new(|_| SliderState::new().min(0.).max(100.).default_value(50.));
        cx.subscribe(&slider, |this, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(value) => {
                this.changes.push(value.start());
                cx.notify();
            }
        })
        .detach();

        Self {
            slider,
            changes: Vec::new(),
        }
    }

    fn value(&self, cx: &gpui::App) -> f32 {
        self.slider.read(cx).value().start()
    }
}

impl Render for SliderHarness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p(px(16.))
            .child(div().w(px(240.)).h(px(40.)).child(Slider::new(&self.slider)))
    }
}

fn init_test(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
}

#[gpui::test]
fn slider_harness_renders(cx: &mut TestAppContext) {
    init_test(cx);
    let window = cx.add_window(|_, cx| SliderHarness::new(cx));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();

    window
        .update(&mut visual, |view, _, cx| {
            assert_eq!(view.value(cx), 50.0);
            assert!(view.changes.is_empty());
        })
        .unwrap();
}

#[gpui::test]
fn slider_click_emits_change(cx: &mut TestAppContext) {
    init_test(cx);
    let window = cx.add_window(|_, cx| SliderHarness::new(cx));
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();

    // Slider rendered inside p(16) with w(240), h(40); y=36 lands on the bar.
    // Clicking near the right side should raise value above the 50.0 default.
    let right_side = point(px(220.), px(36.));
    visual.simulate_mouse_down(right_side, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(right_side, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    window
        .update(&mut visual, |view, _, cx| {
            assert!(
                !view.changes.is_empty(),
                "expected SliderEvent::Change during click interaction"
            );
            assert!(
                view.value(cx) > 50.0,
                "expected slider value to increase after clicking right side, got {}",
                view.value(cx)
            );
        })
        .unwrap();
}