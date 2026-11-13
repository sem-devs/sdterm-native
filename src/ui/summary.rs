//! The resource summary at the bottom of the sidebar: four lines with a small chart each.

use gpui::prelude::*;
use gpui::{AnyElement, Entity, FontWeight, div, px};

use super::Snapshot;
use super::charts::chart;
use super::parts::{Icon, icon, tip};
use crate::metrics::{Metric, WidgetKind};
use crate::monitors;
use crate::resources::Sample;
use crate::state::AppState;
use crate::theme::Theme;

fn row(
    metric: Metric,
    kind: WidgetKind,
    snap: &Snapshot,
    theme: &Theme,
    state: &Entity<AppState>,
) -> AnyElement {
    let label = match metric {
        Metric::Cpu => "CPU",
        Metric::Ram => "RAM",
        Metric::Disk => "Disk",
        Metric::Net => "Net",
        Metric::CpuRam => "CPU",
    };
    let latest: Option<&Sample> = snap.samples.last();
    let value = metric.compact(latest, snap.network_unit);
    let max = metric.scale_max(&snap.samples);
    let series = monitors::series_for(metric, &snap.samples, theme);
    let (muted, fg) = (theme.muted_foreground, theme.foreground);
    let state = state.clone();

    div()
        .id(label)
        .group("res-row")
        .flex()
        .items_center()
        .gap_2()
        .h(px(32.))
        .px_2()
        .rounded_md()
        .child(div().w(px(32.)).flex_none().text_size(px(12.)).text_color(muted).child(label))
        .child(div().w(px(76.)).flex_none().flex().justify_end().text_size(px(12.)).child(value))
        .child(div().flex_1().h(px(18.)).child(chart(series, max, None)))
        .child(
            div()
                .id((label, 1usize))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(18.))
                .rounded_sm()
                .cursor_pointer()
                .text_color(muted)
                .opacity(0.0)
                .group_hover("res-row", |s| s.opacity(1.0))
                .hover(move |s| s.text_color(fg))
                .tooltip(tip("Open in its own window"))
                .child(icon(Icon::ArrowSquareOut, 14.))
                .on_click(move |_, _, cx| monitors::open(cx, kind, &state)),
        )
        .into_any_element()
}

pub fn resources_summary(theme: &Theme, snap: &Snapshot, state: &Entity<AppState>) -> AnyElement {
    let shown = &snap.settings.resources.show_in_sidebar;
    let mut rows = div().flex().flex_col().pb_1();
    for (on, metric, kind) in [
        (shown.cpu, Metric::Cpu, WidgetKind::Cpu),
        (shown.ram, Metric::Ram, WidgetKind::Ram),
        (shown.disk, Metric::Disk, WidgetKind::Disk),
        (shown.net, Metric::Net, WidgetKind::Net),
    ] {
        if on {
            rows = rows.child(row(metric, kind, snap, theme, state));
        }
    }

    let (summary_state, processes_state) = (state.clone(), state.clone());
    let (muted, fg) = (theme.muted_foreground, theme.foreground);
    div()
        .flex_none()
        .border_t_1()
        .border_color(theme.border)
        .pt_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pl_4()
                .pr_2()
                .pb_1()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child("Resources"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(px(11.))
                        .child(
                            div()
                                .id("open-processes")
                                .px_1p5()
                                .rounded_sm()
                                .cursor_pointer()
                                .text_color(muted)
                                .hover(move |s| s.text_color(fg))
                                .child("Processes")
                                .on_click(move |_, _, cx| {
                                    monitors::open(cx, WidgetKind::Processes, &processes_state)
                                }),
                        )
                        .child(
                            div()
                                .id("open-summary")
                                .px_1p5()
                                .rounded_sm()
                                .cursor_pointer()
                                .text_color(muted)
                                .hover(move |s| s.text_color(fg))
                                .tooltip(tip("Open all in one window"))
                                .child(icon(Icon::ArrowSquareOut, 14.))
                                .on_click(move |_, _, cx| {
                                    monitors::open(cx, WidgetKind::Summary, &summary_state)
                                }),
                        ),
                ),
        )
        .child(rows)
        .into_any_element()
}
