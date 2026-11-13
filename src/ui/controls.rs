//! Form controls for the settings page: sections, rows, switches, choices, steppers.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, App, ElementId, FontWeight, IntoElement, SharedString, div, px};

use super::parts::{Icon, icon};
use crate::theme::Theme;

pub fn section(
    theme: &Theme,
    title: &'static str,
    description: Option<&'static str>,
    rows: Vec<AnyElement>,
) -> AnyElement {
    let mut body = div().flex().flex_col();
    for row in rows {
        body = body.child(row);
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .mb_8()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .pb_2()
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .when_some(description, |d, text| {
                    d.child(div().text_color(theme.muted_foreground).max_w(px(560.)).child(text))
                }),
        )
        .child(body)
        .into_any_element()
}

pub fn row(
    theme: &Theme,
    label: &'static str,
    hint: Option<&'static str>,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_8()
        .py_3()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w(px(0.))
                .child(div().font_weight(FontWeight::MEDIUM).child(label))
                .when_some(hint, |d, text| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .max_w(px(460.))
                            .child(text),
                    )
                }),
        )
        .child(div().flex_none().child(control))
        .into_any_element()
}

pub fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    theme: &Theme,
    on_change: impl Fn(bool, &mut App) + 'static,
) -> impl IntoElement {
    let knob = div()
        .absolute()
        .top(px(2.))
        .left(px(if on { 18. } else { 2. }))
        .size(px(16.))
        .rounded_full()
        .bg(if on { theme.primary_foreground } else { theme.foreground });
    div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(36.))
        .h(px(20.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { theme.primary } else { theme.input })
        .child(knob)
        .on_click(move |_, _, cx| on_change(!on, cx))
}

/// A row of buttons of which one is chosen.
pub fn choice<T: Copy + PartialEq + 'static>(
    prefix: &'static str,
    options: &[(T, &'static str)],
    current: T,
    theme: &Theme,
    on_pick: impl Fn(T, &mut App) + 'static,
) -> impl IntoElement {
    let on_pick = Rc::new(on_pick);
    let mut group = div().flex().gap_1().p_0p5().rounded_lg().bg(theme.muted);
    for (i, (value, label)) in options.iter().enumerate() {
        let value = *value;
        let on_pick = on_pick.clone();
        let selected = value == current;
        let hover = theme.accent;
        group = group.child(
            div()
                .id((prefix, i))
                .flex()
                .items_center()
                .h(px(26.))
                .px_3()
                .rounded_md()
                .cursor_pointer()
                .text_size(px(12.))
                .when(selected, |d| d.bg(theme.accent).text_color(theme.foreground).font_weight(FontWeight::MEDIUM))
                .when(!selected, |d| {
                    d.text_color(theme.muted_foreground).hover(move |s| s.bg(hover))
                })
                .child(*label)
                .on_click(move |_, _, cx| on_pick(value, cx)),
        );
    }
    group
}

/// `−  value  +`.
pub fn stepper(
    prefix: &'static str,
    value: impl Into<SharedString>,
    theme: &Theme,
    on_decrease: impl Fn(&mut App) + 'static,
    on_increase: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let step = |id: (&'static str, usize), glyph: Icon, f: Box<dyn Fn(&mut App)>| {
        let hover = theme.accent;
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded_md()
            .cursor_pointer()
            .bg(theme.muted)
            .hover(move |s| s.bg(hover))
            .child(icon(glyph, 14.))
            .on_click(move |_, _, cx| f(cx))
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(step((prefix, 0), Icon::Minus, Box::new(on_decrease)))
        .child(div().w(px(48.)).flex().justify_center().child(value.into()))
        .child(step((prefix, 1), Icon::Plus, Box::new(on_increase)))
}
