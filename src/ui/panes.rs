//! The main area: title bar, layout menu, and the terminals with their headers.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, CursorStyle, Entity, FontWeight, MouseButton, SharedString,
    WindowControlArea, div, px, relative, transparent_black,
};
use gpui_terminal::TerminalView;

use super::parts::{
    DividerDrag, DragGhost, Icon, NoGhost, PaneDrag, color_chip, icon, icon_button, icon_hover, menu_panel,
    mini_icon_button, mix, overlay, text_button, tip,
};
use super::{MAC, Snapshot, shortcut};
use crate::layout::{self, Axis, Node};
use crate::model::{LAYOUTS, LayoutId, PANE_COLORS, Pane, PaneColor, Workspace};
use crate::state::{AppState, Edit, Menu, View, pane_label};
use crate::text_input::TextInput;
use crate::updater::UpdateStatus;
use crate::theme::Theme;

pub fn main_area(snap: Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let title = match (&snap.view, &snap.active) {
        (View::Settings, _) => "Settings".to_string(),
        (_, Some(ws)) => ws.name.clone(),
        _ => String::new(),
    };
    let parked = snap.active.as_ref().is_some_and(|w| w.parked);
    let show_tools = snap.view == View::Terminals && !parked && snap.active.is_some();

    let new_pane = state.clone();
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
                    d.child(icon_button("show-sidebar", Icon::Sidebar, format!("Show sidebar ({})", shortcut("B")), theme, move |cx| {
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
                    .child(layout_button(&active, &snap, theme, state))
                    .child(icon_button("new-terminal", Icon::Plus, format!("New terminal ({})", shortcut("T")), theme, move |cx| {
                        new_pane.update(cx, |s, cx| s.add_pane(cx))
                    })),
            )
        });

    let body: AnyElement = if snap.view == View::Settings {
        super::settings::settings_view(&snap, theme, state)
    } else {
        match &snap.active {
            None => empty_state(
                theme,
                "No workspaces in this window",
                "Create one to get started.",
                "New workspace",
                {
                    let s = state.clone();
                    move |cx| s.update(cx, |s, cx| s.add_workspace(cx))
                },
            ),
            Some(ws) if ws.parked => parked_view(ws, &snap, theme, state),
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
        .children(update_banner(&snap, theme, state))
        .child(div().flex_1().min_h(px(0.)).child(body))
        .into_any_element()
}

/// Tells you a new version is out. It sits under the title bar, in the first window only.
fn update_banner(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> Option<AnyElement> {
    if !snap.shows_update {
        return None;
    }
    let (message, actions): (String, Vec<AnyElement>) = match &snap.update {
        UpdateStatus::Available { version, .. } => {
            let (install, later) = (state.clone(), state.clone());
            let label = if cfg!(target_os = "macos") { "Update and restart" } else { "Download" };
            (
                format!("sdterm {version} is available."),
                vec![
                    text_button("update-now", label, theme, true, move |cx| {
                        install.update(cx, |s, cx| s.install_update(cx))
                    })
                    .into_any_element(),
                    text_button("update-later", "Later", theme, false, move |cx| {
                        later.update(cx, |s, cx| s.dismiss_update(cx))
                    })
                    .into_any_element(),
                ],
            )
        }
        UpdateStatus::Downloading(n) => (format!("Downloading the update: {n}%"), vec![]),
        UpdateStatus::Error(message) => {
            let dismiss = state.clone();
            (
                format!("The update did not work: {message}"),
                vec![text_button("update-dismiss", "Dismiss", theme, false, move |cx| {
                    dismiss.update(cx, |s, cx| s.dismiss_update(cx))
                })
                .into_any_element()],
            )
        }
        _ => return None,
    };
    Some(
        div()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .h(px(40.))
            .px_4()
            .bg(theme.primary.opacity(0.12))
            .border_b_1()
            .border_color(theme.border)
            .child(div().min_w(px(0.)).truncate().child(message))
            .child(div().flex().flex_none().gap_2().children(actions))
            .into_any_element(),
    )
}

// ---- Layout menu ---------------------------------------------------------------------------

/// A miniature of a layout, like the previews of window-snapping tools.
fn thumb(node: &Node, theme: &Theme) -> AnyElement {
    match node {
        Node::Pane(_) => div()
            .size_full()
            .rounded(px(2.))
            .bg(theme.muted_foreground.opacity(0.45))
            .into_any_element(),
        Node::Open => div()
            .size_full()
            .rounded(px(2.))
            .border_1()
            .border_dashed()
            .border_color(theme.muted_foreground.opacity(0.6))
            .into_any_element(),
        Node::Split { axis, children, .. } => {
            let mut d = div().size_full().flex().gap_px();
            d = match axis {
                Axis::Horizontal => d.flex_row(),
                Axis::Vertical => d.flex_col(),
            };
            for (weight, child) in children {
                let mut cell = div().min_w(px(0.)).min_h(px(0.)).flex_basis(relative(0.)).child(thumb(child, theme));
                cell.style().flex_grow = Some(*weight);
                d = d.child(cell);
            }
            d.into_any_element()
        }
    }
}

fn layout_button(active: &Workspace, snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let open = snap.menu == Some(Menu::Layout);
    let toggle = state.clone();
    let (bg, fg) = (theme.accent, theme.foreground);
    let button = div()
        .id("layout-button")
        .flex()
        .items_center()
        .gap_1p5()
        .h(px(28.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(theme.muted_foreground)
        .group("layout-button")
        .hover(move |s| s.bg(bg).text_color(fg))
        .tooltip(tip("Arrange terminals"))
        .child(icon_hover(Icon::SquaresFour, 16., theme.muted_foreground, fg, "layout-button"))
        .child(active.layout.label())
        .on_click(move |_, _, cx| toggle.update(cx, |s, cx| s.toggle_menu(Menu::Layout, cx)));

    let mut holder = div().relative().child(button);
    if open {
        let mut grid = div().flex().flex_wrap().gap_1();
        for (i, layout) in LAYOUTS.iter().enumerate() {
            let layout: LayoutId = *layout;
            let state = state.clone();
            let ws_id = active.id.clone();
            let selected = active.layout == layout;
            // Quarters is the one layout that also works with fewer terminals: show it with two.
            let preview = layout::compute(layout, if layout == LayoutId::Quarters { 2 } else { 4 }, None);
            let hover_bg = theme.accent;
            grid = grid.child(
                div()
                    .id(("layout", i))
                    .w(px(136.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_1p5()
                    .rounded_md()
                    .border_1()
                    .border_color(if selected { theme.primary } else { transparent_black() })
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .child(div().w(px(44.)).h(px(30.)).flex_none().child(thumb(&preview, theme)))
                    .child(div().text_size(px(12.)).child(layout.label()))
                    .on_click(move |_, _, cx| {
                        state.update(cx, |s, cx| {
                            s.close_menu(cx);
                            s.set_layout(&ws_id, layout, cx)
                        })
                    }),
            );
        }
        holder = holder.child(overlay(
            menu_panel(theme)
                .top(px(32.))
                .right_0()
                .w(px(300.))
                .p_2()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child("Pick a layout. Drag a terminal's title onto another to swap them, and drag the line between them to resize."),
                )
                .child(grid),
        ));
    }
    holder.into_any_element()
}

// ---- Empty and parked states ---------------------------------------------------------------

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
        .child(icon(Icon::TerminalWindow, 32., theme.muted_foreground))
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

fn parked_view(ws: &Workspace, snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let id = ws.id.clone();
    let launch_state = state.clone();
    let count = ws.panes.len();
    let folder_name = if MAC { "Finder" } else { "Explorer" };
    let mut list = div().flex().flex_col().mt_2().max_w(px(640.)).border_t_1().border_color(theme.border);
    for (i, pane) in ws.panes.iter().enumerate() {
        let folder = snap.pane_folders.get(&pane.id).cloned().or_else(|| {
            (!ws.folder.is_empty()).then(|| ws.folder.clone())
        });
        let reveal = folder.clone().map(|folder| {
            mini_icon_button(("parked-folder", i), Icon::FolderOpen, format!("Show folder in {folder_name}"), theme, move |cx| {
                cx.reveal_path(std::path::Path::new(&folder))
            })
        });
        list = list.child(
            div()
                .flex()
                .items_start()
                .gap_3()
                .py_3()
                .border_b_1()
                .border_color(theme.border)
                .child(div().mt(px(4.)).child(color_chip(theme, pane.color)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .child(div().font_weight(FontWeight::MEDIUM).child(pane_label(pane, i)))
                        .when(!pane.note.trim().is_empty(), |d| {
                            d.child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(pane.note.clone()))
                        })
                        .when_some(folder, |d, folder| {
                            d.child(div().truncate().text_size(px(11.)).text_color(theme.muted_foreground).child(folder))
                        }),
                )
                .children(reveal),
        );
    }
    let (primary, primary_fg) = (theme.primary, theme.primary_foreground);
    let launch = div()
        .id("launch")
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(px(30.))
        .px_3()
        .rounded_md()
        .cursor_pointer()
        .bg(primary)
        .text_color(primary_fg)
        .hover(move |s| s.bg(primary.opacity(0.85)))
        .child(icon(Icon::Play, 14., primary_fg))
        .child(format!("Launch {count} {}", if count == 1 { "terminal" } else { "terminals" }))
        .on_click(move |_, _, cx| {
            if count > 0 {
                launch_state.update(cx, |s, cx| s.set_parked(&id, false, cx))
            }
        });
    div()
        .id("parked-view")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .items_start()
        .gap_4()
        .px(px(48.))
        .py(px(40.))
        .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(ws.name.clone()))
        .child(
            div()
                .max_w(px(480.))
                .text_color(theme.muted_foreground)
                .child("Parked. Its terminals are stopped, so it costs nothing. Names, notes, colors and folders are kept, and every terminal starts again where it left off."),
        )
        .child(launch)
        .child(list)
        .into_any_element()
}

// ---- Terminals -----------------------------------------------------------------------------

/// A labeled part of the details editor.
fn field(theme: &Theme, label: &'static str, content: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(theme.foreground).child(label))
        .child(content)
}

/// What opens from the title of a terminal: its name, a note about it and its color, in one place.
fn details_editor(
    pane: &Pane,
    name: Entity<TextInput>,
    note: Entity<TextInput>,
    theme: &Theme,
    state: &Entity<AppState>,
) -> AnyElement {
    let mut swatches = div().flex().flex_wrap().gap_1p5();
    {
        let (state, id) = (state.clone(), pane.id.clone());
        let none = pane.color.is_none();
        let (fg, muted, border, hover) = (theme.foreground, theme.muted_foreground, theme.input, theme.accent);
        swatches = swatches.child(
            div()
                .id("no-color")
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.))
                .rounded_md()
                .border_1()
                .border_color(if none { fg } else { border })
                .text_color(if none { fg } else { muted })
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .tooltip(tip("No color"))
                .child(icon(Icon::Prohibit, 14., if none { fg } else { muted }))
                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.set_pane_color(&id, None, cx))),
        );
    }
    for (i, color) in PANE_COLORS.iter().enumerate() {
        let color: PaneColor = *color;
        let (state, id) = (state.clone(), pane.id.clone());
        swatches = swatches.child(
            div()
                .id(("swatch", i))
                .size(px(24.))
                .rounded_md()
                .cursor_pointer()
                .bg(theme.pane(Some(color)))
                .border_2()
                .border_color(if pane.color == Some(color) { theme.foreground } else { transparent_black() })
                .tooltip(tip(color.label()))
                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.set_pane_color(&id, Some(color), cx))),
        );
    }
    overlay(
        menu_panel(theme)
            .top(px(34.))
            .left(px(8.))
            .w(px(320.))
            .p_3()
            .gap_3()
            .child(field(theme, "Name", name))
            .child(field(theme, "Note", note))
            .child(field(theme, "Color", swatches))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .child("Saved as you type. Esc or a click outside closes this."),
            ),
    )
    .into_any_element()
}

fn pane_frame(
    index: usize,
    pane: &Pane,
    view: Entity<TerminalView>,
    focused: bool,
    snap: &Snapshot,
    theme: &Theme,
    state: &Entity<AppState>,
) -> AnyElement {
    let accent = theme.pane(pane.color);
    let (fg, muted) = (theme.foreground, theme.muted_foreground);
    let header_bg = if focused { mix(theme.muted, accent, 0.14) } else { theme.muted };
    let label = pane_label(pane, index);

    let fields = snap.pane_editor.as_ref().filter(|f| f.id == pane.id);
    let folder_name = if MAC { "Finder" } else { "Explorer" };

    let (focus_state, close_state) = (state.clone(), state.clone());
    let (focus_id, close_id) = (pane.id.clone(), pane.id.clone());

    // The title: click it to change the name, the note and the color.
    let first_line = pane.note.lines().next().unwrap_or("").to_string();
    let open_state = state.clone();
    let open_id = pane.id.clone();
    let hover_bg = theme.accent.opacity(0.5);
    let title = div()
        .id(("pane-title", index))
        .flex()
        .flex_1()
        .items_center()
        .gap_2()
        .h_full()
        .min_w(px(0.))
        .px_1()
        .rounded_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .tooltip(tip(if pane.note.trim().is_empty() {
            "Edit name, note and color".to_string()
        } else {
            pane.note.clone()
        }))
        .child(color_chip(theme, pane.color))
        .child(
            div()
                .flex_none()
                .font_weight(if focused { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                .child(label.clone()),
        )
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .text_color(if first_line.is_empty() { muted.opacity(0.55) } else { muted })
                .child(if first_line.is_empty() { "Add a note".to_string() } else { first_line }),
        )
        .on_click(move |_, _, cx| {
            open_state.update(cx, |s, cx| s.begin_edit(Edit::PaneDetails(open_id.clone()), cx))
        });

    // Open the terminal's folder in Finder or Explorer.
    let reveal_state = state.clone();
    let reveal_id = pane.id.clone();
    let reveal = mini_icon_button(
        ("pane-folder", index),
        Icon::FolderOpen,
        format!("Show folder in {folder_name}"),
        theme,
        move |cx| {
            let folder = reveal_state.read(cx).pane_folder(&reveal_id);
            if let Some(folder) = folder {
                cx.reveal_path(&folder);
            }
        },
    );

    let close = mini_icon_button(
        ("close-pane", index),
        Icon::X,
        format!("Close terminal ({})", shortcut("W")),
        theme,
        move |cx| close_state.update(cx, |s, cx| s.close_pane(&close_id, cx)),
    );

    let drag = PaneDrag { id: pane.id.clone(), label: label.clone(), color: accent };
    let (drop_state, drop_id) = (state.clone(), pane.id.clone());
    let primary = theme.primary;

    let header = div()
        .id(("pane-header", index))
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .pl_1()
        .pr_1()
        .bg(header_bg)
        .text_size(px(12.))
        .text_color(if focused { fg } else { muted })
        .cursor(CursorStyle::OpenHand)
        .on_drag(drag, |drag, _, _, cx| {
            cx.new(|_| DragGhost { label: drag.label.clone(), color: drag.color })
        })
        .child(title)
        .child(div().flex().flex_none().items_center().child(reveal).child(close));

    let mut frame = div()
        .id(("pane", index))
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .border_1()
        .border_color(if focused { accent.opacity(0.55) } else { transparent_black() })
        // Dropping another terminal here swaps the two.
        .drag_over::<PaneDrag>(move |s, _, _, _| s.border_color(primary))
        .on_drop(move |drag: &PaneDrag, _, cx| {
            if drag.id != drop_id {
                drop_state.update(cx, |s, cx| s.swap_panes(&drag.id, &drop_id, cx));
            }
        })
        // A click anywhere on the terminal, its title included, moves the focus there.
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            focus_state.update(cx, |s, cx| s.focus_pane_and_select(&focus_id, cx))
        })
        .child(div().h(px(2.)).flex_none().bg(accent.opacity(if focused { 1.0 } else { 0.4 })))
        .child(header)
        .child(div().flex_1().min_h(px(0.)).overflow_hidden().child(view));

    if let Some(fields) = fields {
        frame = frame.child(details_editor(pane, fields.name.clone(), fields.note.clone(), theme, state));
    }
    frame.into_any_element()
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
                .group("open-slot")
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
                .child(icon_hover(Icon::Plus, 20., muted, fg, "open-slot"))
                .child("New terminal")
                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.add_pane(cx))),
        )
        .into_any_element()
}

fn build(
    node: &Node,
    ws_id: &str,
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
        Node::Split { axis, divider, children } => {
            let gap = mix(theme.background, theme.foreground, 0.10);
            let mut split = div().size_full().flex().gap_px().bg(gap);
            split = match axis {
                Axis::Horizontal => split.flex_row(),
                Axis::Vertical => split.flex_col(),
            };
            for (weight, child) in children {
                let child = build(child, ws_id, frames, open_count, theme, state);
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
            let Some(divider) = *divider else { return split.into_any_element() };

            // The line you can drag to resize the two regions.
            let horizontal = *axis == Axis::Horizontal;
            let payload = DividerDrag { workspace: ws_id.to_string(), divider };
            let (reset_state, reset_id) = (state.clone(), ws_id.to_string());
            let primary = theme.primary;
            let mut handle = div()
                .id(SharedString::from(format!("divider-{ws_id}")))
                .group("divider")
                .absolute()
                .flex()
                .items_center()
                .justify_center()
                .on_drag(payload, |_, _, _, cx| cx.new(|_| NoGhost))
                .on_click(move |ev, _, cx| {
                    if ev.click_count() >= 2 {
                        reset_state.update(cx, |s, cx| s.reset_split(&reset_id, cx));
                    }
                });
            let line = div().bg(transparent_black()).group_hover("divider", move |s| s.bg(primary.opacity(0.6)));
            handle = if horizontal {
                handle
                    .top_0()
                    .bottom_0()
                    .w(px(8.))
                    .left(relative(divider.at))
                    .ml(px(-4.))
                    .cursor(CursorStyle::ResizeLeftRight)
                    .child(line.w(px(2.)).h_full())
            } else {
                handle
                    .left_0()
                    .right_0()
                    .h(px(8.))
                    .top(relative(divider.at))
                    .mt(px(-4.))
                    .cursor(CursorStyle::ResizeUpDown)
                    .child(line.h(px(2.)).w_full())
            };
            let drag_state = state.clone();
            let axis = *axis;
            div()
                .id(SharedString::from(format!("split-{ws_id}")))
                .relative()
                .size_full()
                .on_drag_move::<DividerDrag>(move |ev, _, cx| {
                    let drag = ev.drag(cx).clone();
                    let (pos, origin, extent) = match axis {
                        Axis::Horizontal => (ev.event.position.x, ev.bounds.origin.x, ev.bounds.size.width),
                        Axis::Vertical => (ev.event.position.y, ev.bounds.origin.y, ev.bounds.size.height),
                    };
                    let pointer: f32 = (pos - origin) / extent;
                    let value = layout::split_from_pointer(drag.divider, pointer);
                    drag_state.update(cx, |s, cx| s.set_split(&drag.workspace, value, cx));
                })
                .child(split)
                .child(handle)
                .into_any_element()
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
                pane_frame(i, pane, view.clone(), snap.focused.contains(&pane.id), snap, theme, state)
            })
        })
        .collect();
    let tree = layout::compute(ws.layout, ws.panes.len(), ws.split);
    let mut open = 0;
    build(&tree, &ws.id, &mut frames, &mut open, theme, state)
}
