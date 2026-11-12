//! The pop-out windows: resource charts and the processes list.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, FontWeight, Render, Subscription,
    TitlebarOptions, Window, WindowKind, WindowOptions, div, px, size,
};
use parking_lot::Mutex;

use crate::metrics::{Metric, WidgetKind};
use crate::model::NetworkUnit;
use crate::resources::{ProcessInfo, ResourceFeed, Sample, Sampler, format_bytes};
use crate::state::AppState;
use crate::theme::Theme;
use crate::ui::charts::{ChartSeries, chart};

const UI_FONT: &str = ".SystemUIFont";

/// The chart series of a metric for the samples so far.
pub fn series_for(metric: Metric, samples: &[Sample], theme: &Theme) -> Vec<ChartSeries> {
    metric
        .series()
        .into_iter()
        .map(|def| ChartSeries {
            values: samples.iter().map(|s| (def.pick)(s)).collect(),
            color: if def.primary { theme.primary } else { theme.muted_foreground },
            area: def.primary,
        })
        .collect()
}

/// Opens a pop-out window, or brings the one that is already open to the front.
pub fn open(cx: &mut App, kind: WidgetKind, state: &Entity<AppState>) {
    if let Some(handle) = state.read(cx).widgets.get(&kind).copied() {
        if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
            return;
        }
    }
    let (width, height, min_height) = kind.size();
    let on_top = state.read(cx).data.settings.resources.popout_always_on_top;
    let feed = state.read(cx).resources.clone();
    let key = format!("{kind:?}");
    let saved = state.read(cx).data.widget_bounds.get(&key).copied();
    let default = Bounds::centered(None, size(px(width), px(height)), cx);
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(kind.label().into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        window_bounds: Some(crate::ui::opening_bounds(saved, default, cx)),
        kind: if on_top { WindowKind::PopUp } else { WindowKind::Normal },
        window_min_size: Some(size(px(240.), px(min_height))),
        ..Default::default()
    };
    let opened: Option<AnyWindowHandle> = if kind == WidgetKind::Processes {
        let state = state.clone();
        cx.open_window(options, move |window, cx| {
            cx.new(|cx| {
                watch_bounds(&state, key, window, cx);
                ProcessesView::new(state.clone(), cx)
            })
        })
        .ok()
        .map(Into::into)
    } else {
        let state = state.clone();
        cx.open_window(options, move |window, cx| {
            cx.new(|cx| {
                watch_bounds(&state, key, window, cx);
                MonitorView::new(kind, feed, state.clone(), cx)
            })
        })
        .ok()
        .map(Into::into)
    };
    if let Some(handle) = opened {
        state.update(cx, |s, _| {
            s.widgets.insert(kind, handle);
        });
    }
}

/// Saves where a pop-out window is whenever it moves or is resized.
fn watch_bounds<T: 'static>(state: &Entity<AppState>, key: String, window: &mut Window, cx: &mut Context<T>) {
    let state = state.clone();
    cx.observe_window_bounds(window, move |_, window, cx| {
        crate::ui::remember_bounds(&state, true, &key, window, cx);
    })
    .detach();
}

// ---- Charts window -------------------------------------------------------------------------

pub struct MonitorView {
    kind: WidgetKind,
    feed: Entity<ResourceFeed>,
    state: Entity<AppState>,
    _subs: [Subscription; 2],
}

impl MonitorView {
    fn new(kind: WidgetKind, feed: Entity<ResourceFeed>, state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let subs = [
            cx.observe(&feed, |_, _, cx| cx.notify()),
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        Self { kind, feed, state, _subs: subs }
    }
}

impl Render for MonitorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let unit = self.state.read(cx).data.settings.resources.network_unit;
        let samples: Vec<Sample> = self.feed.read(cx).samples.iter().copied().collect();
        let latest = samples.last().copied();
        let metrics = self.kind.metrics();
        let single = metrics.len() == 1;

        let mut cards = div().size_full().flex().flex_col().gap_3().p_3();
        for metric in metrics {
            cards = cards.child(metric_card(metric, &samples, latest.as_ref(), unit, &theme, single));
        }
        div()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .child(cards)
    }
}

fn metric_card(
    metric: Metric,
    samples: &[Sample],
    latest: Option<&Sample>,
    unit: NetworkUnit,
    theme: &Theme,
    fill: bool,
) -> impl IntoElement {
    let max = metric.scale_max(samples);
    let mut legend = div().flex().gap_3().text_size(px(11.)).text_color(theme.muted_foreground);
    let defs = metric.series();
    if defs.len() > 1 {
        for def in &defs {
            let color = if def.primary { theme.primary } else { theme.muted_foreground };
            let value = latest.and_then(|s| (def.pick)(s));
            legend = legend.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().size(px(8.)).rounded_full().bg(color))
                    .child(format!("{} {}", def.label, (def.format)(value, unit))),
            );
        }
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .when(fill, |d| d.flex_1())
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(metric.label()))
                .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(metric.headline(latest, unit))),
        )
        .child(div().text_size(px(11.)).text_color(theme.muted_foreground).child(metric.detail(latest, unit)))
        .child(
            div()
                .flex_1()
                .min_h(px(70.))
                .rounded_md()
                .bg(theme.muted)
                .child(chart(series_for(metric, samples, theme), max, Some(theme.border))),
        )
        .child(legend)
}

// ---- Processes window ----------------------------------------------------------------------

pub struct ProcessesView {
    rows: Vec<ProcessInfo>,
    by_memory: bool,
    state: Entity<AppState>,
    sampler: Arc<Mutex<Sampler>>,
    _task: gpui::Task<()>,
    _observe: Subscription,
}

impl ProcessesView {
    fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let sampler = Arc::new(Mutex::new(Sampler::new()));
        let task = cx.spawn({
            let sampler = sampler.clone();
            async move |this, cx| {
                loop {
                    let by_memory = match this.update(cx, |view, _| view.by_memory) {
                        Ok(b) => b,
                        Err(_) => break,
                    };
                    let sampler = sampler.clone();
                    let rows = cx
                        .background_executor()
                        .spawn(async move { sampler.lock().processes(60, by_memory) })
                        .await;
                    if this.update(cx, |view, cx| {
                        view.rows = rows;
                        cx.notify();
                    })
                    .is_err()
                    {
                        break;
                    }
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                }
            }
        });
        let _observe = cx.observe(&state, |_, _, cx| cx.notify());
        Self { rows: Vec::new(), by_memory: false, state, sampler, _task: task, _observe }
    }
}

impl Render for ProcessesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = (&self.state, &self.sampler);
        let theme = cx.global::<Theme>().clone();
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        let toggle = |id: &'static str, label: &'static str, selected: bool, by_memory: bool| {
            let hover = theme.accent;
            div()
                .id(id)
                .h(px(24.))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .cursor_pointer()
                .text_size(px(12.))
                .when(selected, |d| d.bg(theme.accent).text_color(theme.foreground))
                .when(!selected, |d| d.text_color(theme.muted_foreground).hover(move |s| s.bg(hover)))
                .child(label)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.by_memory = by_memory;
                    cx.notify();
                }))
        };

        let mut table = div().id("process-rows").flex().flex_col().flex_1().overflow_y_scroll();
        for p in &self.rows {
            table = table.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(24.))
                    .px_3()
                    .text_size(px(12.))
                    .child(div().flex_1().truncate().child(p.name.clone()))
                    .child(div().w(px(56.)).text_color(theme.muted_foreground).child(p.pid.to_string()))
                    .child(div().w(px(56.)).flex().justify_end().child(format!("{:.1}%", p.cpu)))
                    .child(div().w(px(76.)).flex().justify_end().child(format_bytes(p.rss))),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(UI_FONT)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Processes"))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme.muted_foreground)
                                    .child(format!("CPU as a percent of one core, like Activity Monitor ({cores} cores)")),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .p_0p5()
                            .rounded_lg()
                            .bg(theme.muted)
                            .child(toggle("by-cpu", "CPU", !self.by_memory, false))
                            .child(toggle("by-mem", "Memory", self.by_memory, true)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(24.))
                    .px_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().child("Name"))
                    .child(div().w(px(56.)).child("PID"))
                    .child(div().w(px(56.)).flex().justify_end().child("CPU"))
                    .child(div().w(px(76.)).flex().justify_end().child("Memory")),
            )
            .child(table)
    }
}
