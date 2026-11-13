//! The sidebar: workspaces, the terminals of the one you are in, the brand and settings.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Entity, FontWeight, PathPromptOptions, WindowControlArea, div, px,
};

use super::parts::{
    Icon, PaneDrag, backdrop, color_chip, icon_button, icon_hover, menu_item, menu_panel, menu_separator,
    menu_title, mini_icon_button, overlay, text_button,
};
use super::{MAC, Snapshot, VERSION, shortcut};
use crate::model::Workspace;
use crate::state::{AppState, Edit, Menu, View, pane_label};
use crate::theme::Theme;

pub fn sidebar(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let running: Vec<&Workspace> = snap.workspaces.iter().filter(|w| !w.parked).collect();
    let parked: Vec<&Workspace> = snap.workspaces.iter().filter(|w| w.parked).collect();
    let active_id = snap.active.as_ref().map(|w| w.id.clone());
    let parked_open = snap.show_parked || parked.iter().any(|w| Some(&w.id) == active_id.as_ref());

    let mut list =
        div().id("workspace-list").flex().flex_col().gap_0p5().p_2().flex_1().overflow_y_scroll();
    // Where each row is, roughly, so its menu knows whether to open up or down.
    let mut y = 76.0_f32;
    for (i, ws) in running.iter().enumerate() {
        list = list.child(workspace_item(("ws", i), ws, y, active_id.as_deref(), snap, theme, state));
        y += 34.0;
        if Some(&ws.id) == active_id.as_ref() {
            list = list.child(pane_list(ws, snap, theme, state));
            y += pane_list_height(ws);
        }
    }
    if !parked.is_empty() {
        let toggle = state.clone();
        let fg = theme.foreground;
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
            .group("parked-toggle")
            .hover(move |s| s.text_color(fg))
            .child(icon_hover(
                if parked_open { Icon::CaretDown } else { Icon::CaretRight },
                12.,
                theme.muted_foreground,
                fg,
                "parked-toggle",
            ))
            .child(format!("Parked  {}", parked.len()))
            .on_click(move |_, _, cx| toggle.update(cx, |s, cx| s.toggle_parked_list(cx)));
        list = list.child(div().border_t_1().border_color(theme.border).mt_2().child(header));
        y += 46.0;
        if parked_open {
            for (i, ws) in parked.iter().enumerate() {
                list = list.child(workspace_item(("parked", i), ws, y, active_id.as_deref(), snap, theme, state));
                y += 34.0;
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
                .child(icon_button("hide-sidebar", Icon::Sidebar, format!("Hide sidebar ({})", shortcut("B")), theme, move |cx| {
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
                .child(icon_button("new-workspace", Icon::Plus, format!("New workspace ({})", shortcut("N")), theme, move |cx| {
                    new_ws.update(cx, |s, cx| s.add_workspace(cx))
                })),
        )
        .child(list)
        .child(super::summary::resources_summary(theme, snap, state))
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
                        .child(brand_logo(theme))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme.muted_foreground)
                                .child(format!("v{VERSION}")),
                        ),
                )
                .child(icon_button("open-settings", Icon::Gear, format!("Settings ({})", if MAC { "\u{2318}," } else { "Ctrl+Shift+," }), theme, move |cx| {
                    settings.update(cx, |s, cx| {
                        let window = s.window_id.clone();
                        let next = if s.view_of(&window) == View::Settings { View::Terminals } else { View::Settings };
                        s.set_view(next, cx)
                    })
                })),
        )
        .into_any_element()
}

fn workspace_item(
    id: (&'static str, usize),
    ws: &Workspace,
    row_y: f32,
    active_id: Option<&str>,
    snap: &Snapshot,
    theme: &Theme,
    state: &Entity<AppState>,
) -> AnyElement {
    let active = active_id == Some(ws.id.as_str());
    let (hover_bg, fg, muted) = (theme.accent.opacity(0.6), theme.foreground, theme.muted_foreground);
    let renaming = snap
        .editor
        .as_ref()
        .filter(|(edit, _)| *edit == Edit::WorkspaceName(ws.id.clone()))
        .map(|(_, input)| input.clone());
    let menu_open = matches!(&snap.menu, Some(Menu::Workspace(w) | Menu::ConfirmDelete(w)) if *w == ws.id);

    // Dropping a terminal on a workspace moves it there.
    let drop_state = state.clone();
    let drop_id = ws.id.clone();
    let accent = theme.primary;

    let mut row = div()
        .id(id)
        .group("ws-row")
        .relative()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .h(px(32.))
        .pl_2()
        .pr_1()
        .rounded_md()
        .cursor_pointer()
        .text_color(if active { fg } else { muted })
        .drag_over::<PaneDrag>(move |s, _, _, _| s.border_1().border_color(accent))
        .on_drop(move |drag: &PaneDrag, _, cx| {
            drop_state.update(cx, |s, cx| s.move_pane(&drag.id, &drop_id, cx))
        });
    row = if active {
        row.bg(theme.accent)
    } else {
        row.hover(move |s| s.bg(hover_bg).text_color(fg))
    };
    if ws.parked && !active {
        row = row.opacity(0.7);
    }

    let click_state = state.clone();
    let click_id = ws.id.clone();
    let name: AnyElement = match renaming {
        Some(input) => div().flex_1().min_w(px(0.)).child(input).into_any_element(),
        None => div().flex_1().min_w(px(0.)).truncate().child(ws.name.clone()).into_any_element(),
    };

    let menu_state = state.clone();
    let menu_id = ws.id.clone();
    let dots = div()
        .id(("ws-menu", id.1 + if ws.parked { 1000 } else { 0 }))
        .group("ws-dots")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(22.))
        .rounded_sm()
        .text_color(muted)
        .opacity(if menu_open { 1.0 } else { 0.0 })
        .group_hover("ws-row", |s| s.opacity(1.0))
        .hover(move |s| s.text_color(fg))
        .child(icon_hover(Icon::Dots, 16., muted, fg, "ws-dots"))
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            menu_state.update(cx, |s, cx| s.toggle_menu(Menu::Workspace(menu_id.clone()), cx))
        });

    // A parked workspace starts again from its row.
    let launch = ws.parked.then(|| {
        let (st, wid) = (state.clone(), ws.id.clone());
        div()
            .flex_none()
            .opacity(0.0)
            .group_hover("ws-row", |s| s.opacity(1.0))
            .child(mini_icon_button(("ws-launch", id.1), Icon::Play, "Launch terminals", theme, move |cx| {
                cx.stop_propagation();
                st.update(cx, |s, cx| s.set_parked(&wid, false, cx))
            }))
    });

    row = row
        .child(name)
        .child(div().flex_none().text_size(px(12.)).child(ws.panes.len().to_string()))
        .children(launch)
        .child(dots)
        .on_click(move |ev, _, cx| {
            if ev.click_count() >= 2 {
                click_state.update(cx, |s, cx| s.begin_edit(Edit::WorkspaceName(click_id.clone()), cx));
            } else {
                click_state.update(cx, |s, cx| s.set_active_workspace(&click_id, cx));
            }
        });

    if menu_open {
        row = row.child(overlay(workspace_menu(ws, row_y, snap, theme, state)));
    }
    row.into_any_element()
}

/// The SEM Devs logo: the wordmark in the text color, the two squares in the brand colors. Small and
/// quiet, as the brand book allows here.
fn brand_logo(theme: &Theme) -> AnyElement {
    let layer = |path: &'static str| gpui::svg().path(path).absolute().top_0().left_0().size_full();
    let blue = gpui::Hsla::from(gpui::rgb(0x1D56B1));
    let cyan = gpui::Hsla::from(gpui::rgb(0x52E5F1));
    div()
        .id("brand")
        .relative()
        .flex_none()
        .w(px(90.))
        .h(px(14.))
        .cursor_pointer()
        .opacity(0.75)
        .hover(|s| s.opacity(1.0))
        .child(layer("brand/wordmark.svg").text_color(theme.foreground.opacity(0.78)))
        .child(layer("brand/icon-back.svg").text_color(blue))
        .child(layer("brand/icon-front.svg").text_color(cyan.opacity(0.89)))
        .on_click(|_, _, cx| cx.open_url("https://sem-devs.com"))
        .into_any_element()
}

fn window_label(id: &str) -> String {
    if id == "main" { "1".to_string() } else { id.trim_start_matches('w').to_string() }
}

fn workspace_menu(ws: &Workspace, row_y: f32, snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let id = ws.id.clone();
    let file_manager = if MAC { "Finder" } else { "Explorer" };
    let mut other: Vec<String> = snap.open_windows.iter().filter(|w| **w != snap.window_id).cloned().collect();
    other.sort();

    // Opens below the row, or above it when there is not enough room under it.
    let height = 300.0 + if ws.folder.is_empty() { 0.0 } else { 56.0 } + 28.0 * other.len() as f32;
    let upward = row_y + 32.0 + height > snap.window_height - 8.0 && row_y > height;
    let mut panel = menu_panel(theme).right(px(4.)).w(px(256.));
    panel = if upward { panel.bottom(px(30.)) } else { panel.top(px(30.)) };

    if matches!(&snap.menu, Some(Menu::ConfirmDelete(_))) {
        let (yes, no) = (state.clone(), state.clone());
        let id = id.clone();
        let count = ws.panes.len();
        return panel
            .gap_2()
            .p_3()
            .child(div().child(format!(
                "Delete \"{}\"? Its {} {} will be stopped.",
                ws.name,
                count,
                if count == 1 { "terminal" } else { "terminals" }
            )))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(text_button("confirm-delete", "Delete", theme, true, move |cx| {
                        yes.update(cx, |s, cx| {
                            s.close_menu(cx);
                            s.delete_workspace(&id, cx)
                        })
                    }))
                    .child(text_button("cancel-delete", "Cancel", theme, false, move |cx| {
                        no.update(cx, |s, cx| s.close_menu(cx))
                    })),
            )
            .into_any_element();
    }

    let s1 = state.clone();
    let rename_id = id.clone();
    panel = panel.child(menu_item("m-rename", "Rename", theme, false, move |cx| {
        s1.update(cx, |s, cx| s.begin_edit(Edit::WorkspaceName(rename_id.clone()), cx))
    }));

    // The folder new terminals of this workspace start in.
    panel = panel.child(menu_title(theme, "Project folder")).child(
        div()
            .px_2()
            .pb_1()
            .text_size(px(11.))
            .text_color(theme.muted_foreground)
            .when(!ws.folder.is_empty(), |d| d.truncate())
            .child(if ws.folder.is_empty() {
                "Not set. New terminals start in your home folder.".to_string()
            } else {
                ws.folder.clone()
            }),
    );
    let s3 = state.clone();
    let folder_id = id.clone();
    panel = panel.child(menu_item(
        "m-folder",
        if ws.folder.is_empty() { "Choose folder\u{2026}" } else { "Change folder\u{2026}" },
        theme,
        false,
        move |cx| {
            s3.update(cx, |s, cx| s.close_menu(cx));
            choose_folder(cx, s3.clone(), folder_id.clone());
        },
    ));
    if !ws.folder.is_empty() {
        let folder = ws.folder.clone();
        let s4 = state.clone();
        panel = panel.child(menu_item("m-reveal", format!("Show in {file_manager}"), theme, false, move |cx| {
            cx.reveal_path(std::path::Path::new(&folder));
            s4.update(cx, |s, cx| s.close_menu(cx));
        }));
        let (s6, clear_id) = (state.clone(), id.clone());
        panel = panel.child(menu_item("m-clear-folder", "Clear folder", theme, false, move |cx| {
            s6.update(cx, |s, cx| {
                s.close_menu(cx);
                s.set_workspace_folder(&clear_id, "", cx)
            })
        }));
    }

    let s2 = state.clone();
    let park_id = id.clone();
    let parked = ws.parked;
    panel = panel.child(menu_title(theme, "Workspace")).child(menu_item(
        "m-park",
        if parked { "Launch terminals" } else { "Park (stop its terminals)" },
        theme,
        false,
        move |cx| {
            s2.update(cx, |s, cx| {
                s.close_menu(cx);
                s.set_parked(&park_id, !parked, cx)
            })
        },
    ));

    // Move the workspace, with its running terminals, to another window.
    panel = panel.child(menu_title(theme, "Move to"));
    for (i, window) in other.iter().enumerate() {
        let (st, wid, win) = (state.clone(), id.clone(), window.clone());
        panel = panel.child(menu_item(("m-move", i), format!("Window {}", window_label(window)), theme, false, move |cx| {
            st.update(cx, |s, cx| s.move_workspace(&wid, &win, cx))
        }));
    }
    let st = state.clone();
    let wid = id.clone();
    panel = panel.child(menu_item("m-move-new", "New window", theme, false, move |cx| {
        let window = st.update(cx, |s, cx| {
            let window = s.new_window_id();
            s.register_window(&window);
            s.move_workspace(&wid, &window, cx);
            window
        });
        super::open_app_window(cx, &st, window);
    }));

    let s5 = state.clone();
    let del_id = id;
    panel
        .child(menu_separator(theme))
        .child(menu_item("m-delete", "Delete\u{2026}", theme, true, move |cx| {
            s5.update(cx, |s, cx| s.open_menu(Menu::ConfirmDelete(del_id.clone()), cx))
        }))
        .into_any_element()
}

/// Asks for a folder and makes it the workspace's project folder.
fn choose_folder(cx: &mut App, state: Entity<AppState>, workspace: String) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Choose the project folder".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = picked.await {
            if let Some(path) = paths.first() {
                let folder = path.to_string_lossy().to_string();
                state.update(cx, |s, cx| s.set_workspace_folder(&workspace, &folder, cx)).ok();
            }
        }
    })
    .detach();
}

/// About how tall the list of terminals under the active workspace is.
fn pane_list_height(ws: &Workspace) -> f32 {
    if ws.parked || ws.panes.is_empty() {
        return 0.0;
    }
    8.0 + ws.panes.iter().map(|p| if p.note.trim().is_empty() { 25.0 } else { 41.0 }).sum::<f32>()
}

/// Which terminal is which and what it was for: name, color and note at a glance.
fn pane_list(ws: &Workspace, snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    if ws.parked || ws.panes.is_empty() {
        return div().into_any_element();
    }
    let _ = snap;
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

/// The click-away layer for whichever menu is open.
pub fn menu_backdrop(snap: &Snapshot, state: &Entity<AppState>) -> Option<AnyElement> {
    if snap.menu.is_none() && snap.pane_editor.is_none() {
        return None;
    }
    let state = state.clone();
    Some(backdrop(move |cx| state.update(cx, |s, cx| s.dismiss_overlays(cx))))
}
