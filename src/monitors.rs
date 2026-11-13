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
use crate::text_input::{InputEvent, TextInput};
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
                ProcessesView::new(state.clone(), window, cx)
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

/// A line of the processes list: one process, or all the processes with the same name.
#[derive(Debug, PartialEq)]
pub struct ProcessRow {
    pub name: String,
    /// The pid, or how many processes share the name when they are grouped.
    pub detail: String,
    pub cpu: f32,
    pub rss: u64,
}

/// The lines to show: filtered by name, grouped by name when asked, the busiest `limit` first.
pub fn process_rows(list: &[ProcessInfo], query: &str, grouped: bool, by_memory: bool, limit: usize) -> Vec<ProcessRow> {
    let query = query.trim().to_lowercase();
    let matching = list.iter().filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query));
    let mut rows: Vec<ProcessRow> = if grouped {
        let mut by_name: Vec<(ProcessRow, usize)> = Vec::new();
        for p in matching {
            match by_name.iter_mut().find(|(row, _)| row.name == p.name) {
                Some((row, count)) => {
                    row.cpu += p.cpu;
                    row.rss += p.rss;
                    *count += 1;
                }
                None => by_name.push((ProcessRow { name: p.name.clone(), detail: String::new(), cpu: p.cpu, rss: p.rss }, 1)),
            }
        }
        by_name
            .into_iter()
            .map(|(mut row, count)| {
                if count > 1 {
                    row.detail = format!("\u{d7}{count}");
                }
                row
            })
            .collect()
    } else {
        matching
            .map(|p| ProcessRow { name: p.name.clone(), detail: p.pid.to_string(), cpu: p.cpu, rss: p.rss })
            .collect()
    };
    if by_memory {
        rows.sort_by_key(|r| std::cmp::Reverse(r.rss));
    } else {
        rows.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    }
    rows.truncate(limit);
    rows
}

pub struct ProcessesView {
    all: Vec<ProcessInfo>,
    by_memory: bool,
    grouped: bool,
    query: String,
    filter: Entity<TextInput>,
    state: Entity<AppState>,
    sampler: Arc<Mutex<Sampler>>,
    _task: gpui::Task<()>,
    _observe: Subscription,
    _filter_sub: Subscription,
}

impl ProcessesView {
    fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sampler = Arc::new(Mutex::new(Sampler::new()));
        let task = cx.spawn({
            let sampler = sampler.clone();
            async move |this, cx| {
                loop {
                    let sampler = sampler.clone();
                    // Everything, busiest first: the view filters, groups and sorts what it shows.
                    let all = cx.background_executor().spawn(async move { sampler.lock().processes(usize::MAX, false) }).await;
                    if this
                        .update(cx, |view, cx| {
                            view.all = all;
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
        let filter = cx.new(|cx| TextInput::new("", "Filter by name", false, false, window, cx));
        let _filter_sub = cx.subscribe_in(&filter, window, |this, _input, event: &InputEvent, _window, cx| {
            if let InputEvent::Changed(text) = event {
                this.query = text.clone();
                cx.notify();
            }
        });
        Self {
            all: Vec::new(),
            by_memory: false,
            grouped: true,
            query: String::new(),
            filter,
            state,
            sampler,
            _task: task,
            _observe,
            _filter_sub,
        }
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
        for p in &process_rows(&self.all, &self.query, self.grouped, self.by_memory, 60) {
            table = table.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(24.))
                    .px_3()
                    .text_size(px(12.))
                    .child(div().flex_1().truncate().child(p.name.clone()))
                    .child(div().w(px(56.)).text_color(theme.muted_foreground).child(p.detail.clone()))
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
                    .gap_2()
                    .px_3()
                    .pb_2()
                    .child(div().flex_1().min_w(px(0.)).child(self.filter.clone()))
                    .child({
                        let (grouped, accent, fg, muted) =
                            (self.grouped, theme.accent, theme.foreground, theme.muted_foreground);
                        div()
                            .id("group-by-name")
                            .flex()
                            .flex_none()
                            .items_center()
                            .h(px(28.))
                            .px_2()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .when(grouped, |d| d.bg(accent).text_color(fg))
                            .when(!grouped, |d| d.text_color(muted).hover(move |s| s.bg(accent)))
                            .child("Group by name")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.grouped = !view.grouped;
                                cx.notify();
                            }))
                    }),
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
                    .child(div().w(px(56.)).child(if self.grouped { "Count" } else { "PID" }))
                    .child(div().w(px(56.)).flex().justify_end().child("CPU"))
                    .child(div().w(px(76.)).flex().justify_end().child("Memory")),
            )
            .child(table)
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcessInfo, process_rows};

    fn p(pid: u32, name: &str, cpu: f32, rss: u64) -> ProcessInfo {
        ProcessInfo { pid, name: name.into(), cpu, rss }
    }

    fn sample() -> Vec<ProcessInfo> {
        vec![p(1, "Chrome", 10.0, 300), p(2, "Chrome", 20.0, 500), p(3, "zsh", 1.0, 50), p(4, "node", 40.0, 100)]
    }

    #[test]
    fn grouping_adds_up_the_processes_with_the_same_name() {
        let rows = process_rows(&sample(), "", true, false, 10);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["node", "Chrome", "zsh"]);
        assert_eq!((rows[1].cpu, rows[1].rss, rows[1].detail.as_str()), (30.0, 800, "\u{d7}2"));
        assert_eq!(rows[0].detail, "");
    }

    #[test]
    fn without_grouping_each_process_is_a_line_with_its_pid() {
        let rows = process_rows(&sample(), "", false, false, 10);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].detail, "4");
    }

    #[test]
    fn the_filter_ignores_case_and_surrounding_spaces() {
        let rows = process_rows(&sample(), "  CHR ", false, true, 10);
        assert_eq!(rows.iter().map(|r| r.detail.as_str()).collect::<Vec<_>>(), ["2", "1"]);
    }

    #[test]
    fn only_the_busiest_are_kept() {
        assert_eq!(process_rows(&sample(), "", false, false, 2).len(), 2);
    }
}
