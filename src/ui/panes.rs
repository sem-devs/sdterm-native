//! The main area: title bar, layout menu, and the terminals with their headers.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, CursorStyle, Entity, FontWeight, MouseButton, SharedString,
    WindowControlArea, div, px, relative, transparent_black,
};
use gpui_terminal::TerminalView;

use super::parts::{
    DividerDrag, DragGhost, NoGhost, PaneDrag, color_chip, icon_button, menu_item, menu_panel,
    menu_title, mix, overlay, text_button,
};
use super::{MAC, Snapshot};
use crate::layout::{self, Axis, Node};
use crate::model::{LAYOUTS, LayoutId, PANE_COLORS, Pane, PaneColor, Workspace};
use crate::state::{AppState, Edit, Menu, View, pane_label};
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
                    .child(layout_button(&active, &snap, theme, state))
                    .child(icon_button("new-terminal", "+", theme, move |cx| {
                        new_pane.update(cx, |s, cx| s.add_pane(cx))
                    })),
            )
        });

    let body: AnyElement = if snap.view == View::Settings {
        super::settings_view(&snap, theme, state)
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
        .h(px(28.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(theme.muted_foreground)
        .hover(move |s| s.bg(bg).text_color(fg))
        .child(format!("Layout: {}", active.layout.label()))
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
    snap: &Snapshot,
    theme: &Theme,
    state: &Entity<AppState>,
) -> AnyElement {
    let accent = theme.pane(pane.color);
    let (fg, muted) = (theme.foreground, theme.muted_foreground);
    let header_bg = if focused { mix(theme.muted, accent, 0.14) } else { theme.muted };
    let label = pane_label(pane, index);

    let editing = |kind: fn(String) -> Edit| {
        snap.editor.as_ref().filter(|(e, _)| *e == kind(pane.id.clone())).map(|(_, i)| i.clone())
    };
    let name_input = editing(Edit::PaneName);
    let note_input = editing(Edit::PaneNote);
    let color_open = snap.menu == Some(Menu::PaneColor(pane.id.clone()));

    let (focus_state, close_state) = (state.clone(), state.clone());
    let (focus_id, close_id) = (pane.id.clone(), pane.id.clone());

    // Name: double-click to rename.
    let name_state = state.clone();
    let name_id = pane.id.clone();
    let name: AnyElement = match name_input {
        Some(input) => div().w(px(170.)).flex_none().child(input).into_any_element(),
        None => div()
            .id(("pane-name", index))
            .flex_none()
            .font_weight(if focused { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
            .child(label.clone())
            .on_click(move |ev, _, cx| {
                if ev.click_count() >= 2 {
                    name_state.update(cx, |s, cx| s.begin_edit(Edit::PaneName(name_id.clone()), cx));
                }
            })
            .into_any_element(),
    };

    // Note: click to write what this terminal is for.
    let first_line = pane.note.lines().next().unwrap_or("").to_string();
    let note_state = state.clone();
    let note_id = pane.id.clone();
    let note = div()
        .id(("pane-note", index))
        .min_w(px(0.))
        .truncate()
        .cursor_pointer()
        .text_color(if first_line.is_empty() { muted.opacity(0.55) } else { muted })
        .child(if first_line.is_empty() { "Add a note".to_string() } else { first_line })
        .on_click(move |_, _, cx| {
            note_state.update(cx, |s, cx| s.begin_edit(Edit::PaneNote(note_id.clone()), cx))
        });

    // Color chip: click to choose another color.
    let chip_state = state.clone();
    let chip_id = pane.id.clone();
    let chip = div()
        .id(("pane-color", index))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(18.))
        .rounded_sm()
        .cursor_pointer()
        .hover({
            let bg = theme.accent;
            move |s| s.bg(bg)
        })
        .child(color_chip(theme, pane.color))
        .on_click(move |_, _, cx| {
            chip_state.update(cx, |s, cx| s.toggle_menu(Menu::PaneColor(chip_id.clone()), cx))
        });
    let mut chip_holder = div().relative().flex_none().child(chip);
    if color_open {
        let mut swatches = div().flex().flex_wrap().gap_1().w(px(148.));
        for (i, color) in PANE_COLORS.iter().enumerate() {
            let color: PaneColor = *color;
            let (state, id) = (state.clone(), pane.id.clone());
            swatches = swatches.child(
                div()
                    .id(("swatch", i))
                    .size(px(22.))
                    .rounded_md()
                    .cursor_pointer()
                    .bg(theme.pane(Some(color)))
                    .border_2()
                    .border_color(if pane.color == Some(color) { theme.foreground } else { transparent_black() })
                    .on_click(move |_, _, cx| {
                        state.update(cx, |s, cx| {
                            s.close_menu(cx);
                            s.set_pane_color(&id, Some(color), cx)
                        })
                    }),
            );
        }
        let (state, id) = (state.clone(), pane.id.clone());
        chip_holder = chip_holder.child(overlay(
            menu_panel(theme)
                .top(px(22.))
                .left_0()
                .p_2()
                .gap_2()
                .child(menu_title(theme, "Color"))
                .child(swatches)
                .child(menu_item("no-color", "No color", theme, false, move |cx| {
                    state.update(cx, |s, cx| {
                        s.close_menu(cx);
                        s.set_pane_color(&id, None, cx)
                    })
                })),
        ));
    }

    // Open the terminal's folder in Finder or Explorer.
    let reveal_state = state.clone();
    let reveal_id = pane.id.clone();
    let reveal = icon_button(("pane-folder", index), "⌂", theme, move |cx| {
        let folder = reveal_state.read(cx).pane_folder(&reveal_id);
        if let Some(folder) = folder {
            cx.reveal_path(&folder);
        }
    });

    let drag = PaneDrag { id: pane.id.clone(), label: label.clone(), color: accent };
    let (drop_state, drop_id) = (state.clone(), pane.id.clone());
    let primary = theme.primary;

    let header = div()
        .id(("pane-header", index))
        .h(px(26.))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .pl_2()
        .pr_1()
        .bg(header_bg)
        .text_size(px(12.))
        .text_color(if focused { fg } else { muted })
        .cursor(CursorStyle::OpenHand)
        .on_drag(drag, |drag, _, _, cx| {
            cx.new(|_| DragGhost { label: drag.label.clone(), color: drag.color })
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_w(px(0.))
                .child(chip_holder)
                .child(name)
                .child(note),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .child(reveal.size(px(22.)))
                .child(
                    div()
                        .id(("close-pane", index))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(22.))
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |s| s.text_color(fg))
                        .child("×")
                        .on_click(move |_, _, cx| {
                            close_state.update(cx, |s, cx| s.close_pane(&close_id, cx))
                        }),
                ),
        );

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

    if let Some(input) = note_input {
        frame = frame.child(overlay(
            menu_panel(theme)
                .top(px(34.))
                .left(px(8.))
                .right(px(8.))
                .p_2()
                .gap_2()
                .child(menu_title(theme, "What is this terminal for?"))
                .child(input)
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.muted_foreground)
                        .child(if cfg!(target_os = "macos") {
                            "Enter for a new line, ⌘Enter to save, Esc to cancel"
                        } else {
                            "Enter for a new line, Ctrl+Enter to save, Esc to cancel"
                        }),
                ),
        ));
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
