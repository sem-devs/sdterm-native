//! The window: sidebar, title bar and the grid of terminals.
//!
//! Views only read a snapshot of the state and call methods on it; they never change data
//! themselves, so everything they do goes through the same checks and the same saving.

use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, Hsla, MouseButton, Render, Rgba,
    SharedString, Subscription, Window, WindowControlArea, actions, div, px, relative,
    transparent_black,
};
use gpui_terminal::TerminalView;

use crate::layout::{self, Axis, Node};
use crate::model::{LAYOUTS, Pane, PaneColor, Workspace};
use crate::state::{AppState, View, pane_label};
use crate::theme::Theme;

actions!(
    sdterm,
    [NewTerminal, CloseTerminal, NewWorkspace, ToggleSidebar, OpenSettings, Quit]
);

const MAC: bool = cfg!(target_os = "macos");
const UI_FONT: &str = ".SystemUIFont";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `t` of `b` over `a`.
fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b): (Rgba, Rgba) = (a.into(), b.into());
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
    .into()
}

// ---- What the views need from the state ----------------------------------------------------

struct Snapshot {
    sidebar_collapsed: bool,
    view: View,
    show_parked: bool,
    workspaces: Vec<Workspace>,
    active: Option<Workspace>,
    views: HashMap<String, Entity<TerminalView>>,
    focused: HashSet<String>,
}

impl Snapshot {
    fn take(state: &AppState, window: &Window, cx: &App) -> Self {
        let views: HashMap<String, Entity<TerminalView>> =
            state.sessions.iter().map(|(id, s)| (id.clone(), s.view.clone())).collect();
        let focused = views
            .iter()
            .filter(|(_, v)| v.read(cx).focus_handle().is_focused(window))
            .map(|(id, _)| id.clone())
            .collect();
        Self {
            sidebar_collapsed: state.sidebar_collapsed,
            view: state.view,
            show_parked: state.show_parked,
            workspaces: state.mine().into_iter().cloned().collect(),
            active: state.active().cloned(),
            views,
            focused,
        }
    }
}

// ---- The root view -------------------------------------------------------------------------

pub struct Root {
    state: Entity<AppState>,
    focus_subs: HashMap<String, Subscription>,
    _observe: Subscription,
}

impl Root {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let _observe = cx.observe(&state, |_, _, cx| cx.notify());
        Self { state, focus_subs: HashMap::new(), _observe }
    }

    /// Keeps the state in step with where the keyboard focus is, and moves it when asked.
    fn sync_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handles: Vec<(String, FocusHandle)> = self
            .state
            .read(cx)
            .sessions
            .iter()
            .map(|(id, s)| (id.clone(), s.view.read(cx).focus_handle().clone()))
            .collect();
        self.focus_subs.retain(|id, _| handles.iter().any(|(h, _)| h == id));
        for (id, handle) in &handles {
            if self.focus_subs.contains_key(id) {
                continue;
            }
            let pane_id = id.clone();
            let sub = cx.on_focus_in(handle, window, move |this, _window, cx| {
                let pane_id = pane_id.clone();
                this.state.update(cx, |state, cx| state.focus_pane(&pane_id, cx));
            });
            self.focus_subs.insert(id.clone(), sub);
        }
        let pending = self.state.update(cx, |state, _| state.pending_focus.take());
        if let Some(id) = pending {
            if let Some((_, handle)) = handles.iter().find(|(h, _)| *h == id) {
                handle.focus(window);
            }
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_focus(window, cx);
        let theme = cx.global::<Theme>().clone();
        let snap = Snapshot::take(self.state.read(cx), window, cx);
        let state = self.state.clone();

        let mut root = div()
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .on_action(cx.listener(|this, _: &NewTerminal, _, cx| {
                this.state.update(cx, |s, cx| s.add_pane(cx))
            }))
            .on_action(cx.listener(|this, _: &NewWorkspace, _, cx| {
                this.state.update(cx, |s, cx| s.add_workspace(cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.state.update(cx, |s, cx| s.toggle_sidebar(cx))
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.state.update(cx, |s, cx| {
                    let next = if s.view == View::Settings { View::Terminals } else { View::Settings };
                    s.set_view(next, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &CloseTerminal, _, cx| {
                this.state.update(cx, |s, cx| {
                    let pane = s.active().and_then(|w| w.active_pane_id.clone());
                    if let Some(pane) = pane {
                        s.close_pane(&pane, cx);
                    }
                })
            }));

        if !snap.sidebar_collapsed {
            root = root.child(sidebar(&snap, &theme, &state));
        }
        root.child(main_area(snap, &theme, &state))
    }
}

// ---- Small building blocks -----------------------------------------------------------------

fn icon_button(
    id: impl Into<gpui::ElementId>,
    glyph: &'static str,
    theme: &Theme,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let (fg, hover_bg, hover_fg) = (theme.muted_foreground, theme.accent, theme.foreground);
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded_md()
        .cursor_pointer()
        .text_color(fg)
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .child(glyph)
        .on_click(move |_, _, cx| on_click(cx))
}

fn text_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    primary: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> impl IntoElement {
    let (bg, fg, hover) = if primary {
        (theme.primary, theme.primary_foreground, theme.primary.opacity(0.85))
    } else {
        (theme.accent, theme.foreground, theme.accent.opacity(0.7))
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(30.))
        .px_3()
        .rounded_md()
        .cursor_pointer()
        .bg(bg)
        .text_color(fg)
        .hover(move |s| s.bg(hover))
        .child(label.into())
        .on_click(move |_, _, cx| on_click(cx))
}

fn color_chip(theme: &Theme, color: Option<PaneColor>) -> impl IntoElement {
    let c = theme.pane(color);
    let mut chip = div().size(px(10.)).flex_none().rounded(px(3.));
    chip = if color.is_some() { chip.bg(c) } else { chip.border_1().border_color(theme.muted_foreground) };
    chip
}

// ---- Sidebar -------------------------------------------------------------------------------

fn sidebar(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let running: Vec<&Workspace> = snap.workspaces.iter().filter(|w| !w.parked).collect();
    let parked: Vec<&Workspace> = snap.workspaces.iter().filter(|w| w.parked).collect();
    let active_id = snap.active.as_ref().map(|w| w.id.clone());
    let parked_open =
        snap.show_parked || parked.iter().any(|w| Some(&w.id) == active_id.as_ref());

    let mut list = div().id("workspace-list").flex().flex_col().gap_0p5().p_2().flex_1().overflow_y_scroll();
    for (i, ws) in running.iter().enumerate() {
        list = list.child(workspace_item(("ws", i), ws, active_id.as_deref(), theme, state));
        if Some(&ws.id) == active_id.as_ref() {
            list = list.child(pane_list(ws, theme, state));
        }
    }
    if !parked.is_empty() {
        let toggle = state.clone();
        let header = div()
            .id("parked-toggle")
            .flex()
            .items_center()
            .gap_1p5()
            .h(px(28.))
            .px_2()
            .mt_2()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .hover({
                let fg = theme.foreground;
                move |s| s.text_color(fg)
            })
            .child(if parked_open { "▾" } else { "▸" })
            .child(format!("Parked  {}", parked.len()))
            .on_click(move |_, _, cx| toggle.update(cx, |s, cx| s.toggle_parked_list(cx)));
        list = list.child(div().border_t_1().border_color(theme.border).mt_2().child(header));
        if parked_open {
            for (i, ws) in parked.iter().enumerate() {
                list = list.child(workspace_item(("parked", i), ws, active_id.as_deref(), theme, state));
            }
        }
    }

    let new_ws = state.clone();
    let hide = state.clone();
    let settings = state.clone();
    div()
        .w(px(240.))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .bg(theme.card)
        .border_r_1()
        .border_color(theme.border)
        .child(
            // Leaves room for the macOS window buttons; draggable like the rest of the title bar.
            div()
                .window_control_area(WindowControlArea::Drag)
                .h(px(40.))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .pl(px(if MAC { 80. } else { 16. }))
                .pr_2()
                .child(div().font_weight(FontWeight::SEMIBOLD).child("sdterm"))
                .child(icon_button("hide-sidebar", "◧", theme, move |cx| {
                    hide.update(cx, |s, cx| s.toggle_sidebar(cx))
                })),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .pl_4()
                .pr_2()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child("Workspaces"),
                )
                .child(icon_button("new-workspace", "+", theme, move |cx| {
                    new_ws.update(cx, |s, cx| s.add_workspace(cx))
                })),
        )
        .child(list)
        .child(
            // The brand is always on screen, small and quiet.
            div()
                .h(px(36.))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .border_t_1()
                .border_color(theme.border)
                .pl_4()
                .pr_1p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("SEM Devs"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.muted_foreground)
                                .child(format!("v{VERSION}")),
                        ),
                )
                .child(icon_button("open-settings", "⚙", theme, move |cx| {
                    settings.update(cx, |s, cx| {
                        let next = if s.view == View::Settings { View::Terminals } else { View::Settings };
                        s.set_view(next, cx)
                    })
                })),
        )
        .into_any_element()
}

fn workspace_item(
    id: (&'static str, usize),
    ws: &Workspace,
    active_id: Option<&str>,
    theme: &Theme,
    state: &Entity<AppState>,
) -> impl IntoElement {
    let active = active_id == Some(ws.id.as_str());
    let (id_str, hover_bg, fg, muted) = (ws.id.clone(), theme.accent.opacity(0.6), theme.foreground, theme.muted_foreground);
    let state = state.clone();
    let mut row = div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .h(px(32.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_color(if active { fg } else { muted });
    row = if active {
        row.bg(theme.accent)
    } else {
        row.hover(move |s| s.bg(hover_bg).text_color(fg))
    };
    if ws.parked && !active {
        row = row.opacity(0.7);
    }
    row.child(div().truncate().child(ws.name.clone()))
        .child(div().text_size(px(12.)).child(ws.panes.len().to_string()))
        .on_click(move |_, _, cx| state.update(cx, |s, cx| s.set_active_workspace(&id_str, cx)))
}

/// Which terminal is which and what it was for: name, color and note at a glance.
fn pane_list(ws: &Workspace, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    if ws.parked || ws.panes.is_empty() {
        return div().into_any_element();
    }
    let mut list = div()
        .flex()
        .flex_col()
        .gap_px()
        .mt_0p5()
        .mb_1p5()
        .ml_3()
        .pl_2()
        .border_l_1()
        .border_color(theme.border);
    for (i, pane) in ws.panes.iter().enumerate() {
        let current = ws.active_pane_id.as_deref() == Some(pane.id.as_str());
        let id = pane.id.clone();
        let state = state.clone();
        let (hover_bg, fg, muted) = (theme.accent.opacity(0.6), theme.foreground, theme.muted_foreground);
        let note = pane.note.lines().next().unwrap_or("").to_string();
        list = list.child(
            div()
                .id(("pane-item", i))
                .flex()
                .items_start()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .text_color(if current { fg } else { muted })
                .hover(move |s| s.bg(hover_bg))
                .child(div().mt(px(3.)).child(color_chip(theme, pane.color)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(pane_label(pane, i)),
                        )
                        .when(!note.is_empty(), |d| {
                            d.child(div().truncate().text_size(px(11.)).text_color(muted).child(note))
                        }),
                )
                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.focus_pane_and_select(&id, cx))),
        );
    }
    list.into_any_element()
}

// ---- Main area -----------------------------------------------------------------------------

fn main_area(snap: Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let title = match (&snap.view, &snap.active) {
        (View::Settings, _) => "Settings".to_string(),
        (_, Some(ws)) => ws.name.clone(),
        _ => String::new(),
    };
    let parked = snap.active.as_ref().is_some_and(|w| w.parked);
    let show_tools = snap.view == View::Terminals && !parked && snap.active.is_some();
    let layout_name = snap.active.as_ref().map(|w| w.layout.label()).unwrap_or("");

    let layout_state = state.clone();
    let pane_state = state.clone();
    let show_sidebar = state.clone();

    let bar = div()
        .window_control_area(WindowControlArea::Drag)
        .h(px(40.))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(theme.border)
        .pl(px(if snap.sidebar_collapsed { if MAC { 80. } else { 8. } } else { 16. }))
        .pr_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w(px(0.))
                .when(snap.sidebar_collapsed, |d| {
                    d.child(icon_button("show-sidebar", "◧", theme, move |cx| {
                        show_sidebar.update(cx, |s, cx| s.toggle_sidebar(cx))
                    }))
                })
                .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(title))
                .when(parked && snap.view == View::Terminals, |d| {
                    d.child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Parked"))
                }),
        )
        .when(show_tools, |d| {
            let active = snap.active.clone().unwrap();
            d.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("layout-cycle")
                            .flex()
                            .items_center()
                            .h(px(28.))
                            .px_2()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .hover({
                                let (bg, fg) = (theme.accent, theme.foreground);
                                move |s| s.bg(bg).text_color(fg)
                            })
                            .child(format!("Layout: {layout_name}"))
                            .on_click(move |_, _, cx| {
                                let next = LAYOUTS
                                    .iter()
                                    .position(|l| *l == active.layout)
                                    .map_or(LAYOUTS[0], |i| LAYOUTS[(i + 1) % LAYOUTS.len()]);
                                layout_state.update(cx, |s, cx| s.set_layout(&active.id, next, cx))
                            }),
                    )
                    .child(icon_button("new-terminal", "+", theme, move |cx| {
                        pane_state.update(cx, |s, cx| s.add_pane(cx))
                    })),
            )
        });

    let body: AnyElement = if snap.view == View::Settings {
        settings_placeholder(theme)
    } else {
        match &snap.active {
            None => empty_state(theme, "No workspaces in this window", "Create one to get started.", "New workspace", {
                let s = state.clone();
                move |cx| s.update(cx, |s, cx| s.add_workspace(cx))
            }),
            Some(ws) if ws.parked => parked_view(ws, theme, state),
            Some(ws) if ws.panes.is_empty() => empty_state(
                theme,
                "No terminals in this workspace",
                "Open a shell to get started. Each terminal keeps running while you switch workspaces.",
                "New terminal",
                {
                    let s = state.clone();
                    move |cx| s.update(cx, |s, cx| s.add_pane(cx))
                },
            ),
            Some(ws) => pane_grid(ws, &snap, theme, state),
        }
    };

    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .child(bar)
        .child(div().flex_1().min_h(px(0.)).child(body))
        .into_any_element()
}

fn empty_state(
    theme: &Theme,
    title: &'static str,
    body: &'static str,
    action: &'static str,
    on_click: impl Fn(&mut App) + 'static,
) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .justify_center()
        .items_start()
        .gap_4()
        .px(px(64.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().max_w(px(420.)).text_color(theme.muted_foreground).child(body)),
        )
        .child(text_button("empty-action", action, theme, true, on_click))
        .into_any_element()
}

fn settings_placeholder(theme: &Theme) -> AnyElement {
    div()
        .size_full()
        .p_8()
        .text_color(theme.muted_foreground)
        .child("Settings come next.")
        .into_any_element()
}

fn parked_view(ws: &Workspace, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let id = ws.id.clone();
    let state = state.clone();
    let mut list = div().flex().flex_col().gap_2().mt_2();
    for (i, pane) in ws.panes.iter().enumerate() {
        list = list.child(
            div()
                .flex()
                .items_start()
                .gap_3()
                .child(div().mt(px(4.)).child(color_chip(theme, pane.color)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(FontWeight::MEDIUM).child(pane_label(pane, i)))
                        .when(!pane.note.trim().is_empty(), |d| {
                            d.child(div().text_color(theme.muted_foreground).child(pane.note.clone()))
                        }),
                ),
        );
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .justify_center()
        .items_start()
        .gap_3()
        .px(px(64.))
        .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(ws.name.clone()))
        .child(
            div()
                .max_w(px(480.))
                .text_color(theme.muted_foreground)
                .child("Parked: its terminals are stopped, but their names, notes and folders are kept."),
        )
        .child(list)
        .child(text_button("launch", "Launch terminals", theme, true, move |cx| {
            state.update(cx, |s, cx| s.set_parked(&id, false, cx))
        }))
        .into_any_element()
}

// ---- Terminals -----------------------------------------------------------------------------

fn pane_frame(
    index: usize,
    pane: &Pane,
    view: Entity<TerminalView>,
    focused: bool,
    theme: &Theme,
    state: &Entity<AppState>,
) -> AnyElement {
    let accent = theme.pane(pane.color);
    let focus_state = state.clone();
    let close_state = state.clone();
    let (focus_id, close_id) = (pane.id.clone(), pane.id.clone());
    let note = pane.note.lines().next().unwrap_or("").to_string();
    let header_bg = if focused { mix(theme.muted, accent, 0.14) } else { theme.muted };
    let (fg, muted) = (theme.foreground, theme.muted_foreground);

    div()
        .id(("pane", index))
        .size_full()
        .flex()
        .flex_col()
        .border_1()
        .border_color(if focused { accent.opacity(0.55) } else { transparent_black() })
        // A click anywhere on the terminal, its title included, moves the focus there.
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            focus_state.update(cx, |s, cx| s.focus_pane_and_select(&focus_id, cx))
        })
        .child(div().h(px(2.)).flex_none().bg(accent.opacity(if focused { 1.0 } else { 0.4 })))
        .child(
            div()
                .h(px(26.))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .bg(header_bg)
                .text_size(px(12.))
                .text_color(if focused { fg } else { muted })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w(px(0.))
                        .child(
                            div()
                                .flex_none()
                                .font_weight(if focused { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                .child(pane_label(pane, index)),
                        )
                        .when(!note.is_empty(), |d| {
                            d.child(div().truncate().text_color(muted).child(note))
                        }),
                )
                .child(
                    div()
                        .id(("close-pane", index))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(18.))
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |s| s.text_color(fg))
                        .child("×")
                        .on_click(move |_, _, cx| {
                            close_state.update(cx, |s, cx| s.close_pane(&close_id, cx))
                        }),
                ),
        )
        .child(div().flex_1().min_h(px(0.)).overflow_hidden().child(view))
        .into_any_element()
}

fn open_slot(theme: &Theme, state: &Entity<AppState>, n: usize) -> AnyElement {
    let state = state.clone();
    let (muted, fg, border) = (theme.muted_foreground, theme.foreground, theme.input);
    div()
        .size_full()
        .p_3()
        .child(
            div()
                .id(("open-slot", n))
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_dashed()
                .border_color(border)
                .cursor_pointer()
                .text_color(muted)
                .hover(move |s| s.text_color(fg))
                .child(div().text_size(px(20.)).child("+"))
                .child("New terminal")
                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.add_pane(cx))),
        )
        .into_any_element()
}

fn build(
    node: &Node,
    frames: &mut Vec<Option<AnyElement>>,
    open_count: &mut usize,
    theme: &Theme,
    state: &Entity<AppState>,
) -> AnyElement {
    match node {
        Node::Pane(i) => frames[*i].take().unwrap_or_else(|| div().into_any_element()),
        Node::Open => {
            *open_count += 1;
            open_slot(theme, state, *open_count)
        }
        Node::Split { axis, children, .. } => {
            let gap = mix(theme.background, theme.foreground, 0.10);
            let mut split = div().size_full().flex().gap_px().bg(gap);
            split = match axis {
                Axis::Horizontal => split.flex_row(),
                Axis::Vertical => split.flex_col(),
            };
            for (weight, child) in children {
                let child = build(child, frames, open_count, theme, state);
                let mut cell = div()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .overflow_hidden()
                    .flex_basis(relative(0.))
                    .bg(theme.background)
                    .child(child);
                cell.style().flex_grow = Some(*weight);
                split = split.child(cell);
            }
            split.into_any_element()
        }
    }
}

fn pane_grid(ws: &Workspace, snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let mut frames: Vec<Option<AnyElement>> = ws
        .panes
        .iter()
        .enumerate()
        .map(|(i, pane)| {
            snap.views.get(&pane.id).map(|view| {
                pane_frame(i, pane, view.clone(), snap.focused.contains(&pane.id), theme, state)
            })
        })
        .collect();
    let tree = layout::compute(ws.layout, ws.panes.len(), ws.split);
    let mut open = 0;
    build(&tree, &mut frames, &mut open, theme, state)
}
