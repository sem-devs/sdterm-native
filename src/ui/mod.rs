//! The window: sidebar, title bar and the grid of terminals.
//!
//! Views only read a snapshot of the state and call methods on it; they never change data
//! themselves, so everything they do goes through the same checks and the same saving.

pub(crate) mod charts;
mod controls;
mod panes;
mod parts;
mod settings;
mod sidebar;
mod summary;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, Render, Subscription, Window, actions, div, px,
};
use gpui_terminal::TerminalView;

use crate::model::{NetworkUnit, Settings, WindowBounds as SavedBounds, Workspace};
use crate::resources::Sample;
use crate::updater::UpdateStatus;
use crate::state::{AppState, Edit, Menu, SettingsSection, View};
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;

actions!(
    sdterm,
    [
        NewWindow,
        NewTerminal,
        CloseTerminal,
        NewWorkspace,
        ToggleSidebar,
        OpenSettings,
        Quit,
        PopOutSummary,
        PopOutCpu,
        PopOutCpuRam,
        PopOutRam,
        PopOutDisk,
        PopOutNet,
        PopOutProcesses
    ]
);

/// The bounds a window opens with: where it was last if that is still on a display, otherwise
/// `default`.
pub(crate) fn opening_bounds(
    saved: Option<SavedBounds>,
    default: gpui::Bounds<gpui::Pixels>,
    cx: &App,
) -> gpui::WindowBounds {
    use gpui::{Bounds, point, size};
    let displays: Vec<SavedBounds> = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            SavedBounds {
                x: f32::from(b.origin.x) as f64,
                y: f32::from(b.origin.y) as f64,
                width: f32::from(b.size.width) as f64,
                height: f32::from(b.size.height) as f64,
                maximized: false,
            }
        })
        .collect();
    match saved.filter(|s| s.is_reachable(&displays)) {
        Some(s) => {
            let bounds = Bounds {
                origin: point(px(s.x as f32), px(s.y as f32)),
                size: size(px(s.width as f32), px(s.height as f32)),
            };
            if s.maximized {
                gpui::WindowBounds::Maximized(bounds)
            } else {
                gpui::WindowBounds::Windowed(bounds)
            }
        }
        None => gpui::WindowBounds::Windowed(default),
    }
}

/// Saves where a window is (and whether it is maximized), so it opens there next time. `widget`
/// is for the pop-out windows, keyed by their kind; app windows are keyed by their id.
pub(crate) fn remember_bounds(state: &Entity<AppState>, widget: bool, key: &str, window: &Window, cx: &mut App) {
    let (maximized, b) = match window.window_bounds() {
        gpui::WindowBounds::Windowed(b) => (false, b),
        gpui::WindowBounds::Maximized(b) => (true, b),
        gpui::WindowBounds::Fullscreen(b) => (false, b),
    };
    let rect = SavedBounds {
        x: f32::from(b.origin.x) as f64,
        y: f32::from(b.origin.y) as f64,
        width: f32::from(b.size.width) as f64,
        height: f32::from(b.size.height) as f64,
        maximized,
    };
    state.update(cx, |s, cx| s.set_bounds(widget, key, rect, cx));
}

pub(crate) const MAC: bool = cfg!(target_os = "macos");
const UI_FONT: &str = ".SystemUIFont";
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---- What the views need from the state ----------------------------------------------------

pub(crate) struct Snapshot {
    /// Height of the window, so a menu opens upward when there is no room below it.
    pub window_height: f32,
    /// The folder each terminal of the active workspace is in, by terminal id (shown while parked).
    pub pane_folders: HashMap<String, String>,
    pub window_id: String,
    pub open_windows: Vec<String>,
    pub update: UpdateStatus,
    /// The window that shows the update banner: the first one.
    pub shows_update: bool,
    pub sidebar_collapsed: bool,
    pub view: View,
    pub show_parked: bool,
    pub workspaces: Vec<Workspace>,
    pub active: Option<Workspace>,
    pub views: HashMap<String, Entity<TerminalView>>,
    pub focused: HashSet<String>,
    pub menu: Option<Menu>,
    pub settings: Settings,
    pub samples: Vec<Sample>,
    pub network_unit: NetworkUnit,
    pub settings_section: SettingsSection,
    pub confirm_reset: bool,
    /// The text fields of the settings page.
    pub setting_inputs: HashMap<&'static str, Entity<TextInput>>,
    /// The text field of whatever is being renamed or written, with what it is for.
    pub editor: Option<(Edit, Entity<TextInput>)>,
    /// The fields of the editor of a terminal's name, note and color, while it is open.
    pub pane_editor: Option<PaneFields>,
}

/// The two text fields of the editor of a terminal's details.
#[derive(Clone)]
pub struct PaneFields {
    pub id: String,
    pub name: Entity<TextInput>,
    pub note: Entity<TextInput>,
}

impl Snapshot {
    fn take(
        state: &AppState,
        window_id: &str,
        editor: Option<(Edit, Entity<TextInput>)>,
        pane_editor: Option<PaneFields>,
        setting_inputs: HashMap<&'static str, Entity<TextInput>>,
        window: &Window,
        cx: &App,
    ) -> Self {
        let views: HashMap<String, Entity<TerminalView>> =
            state.sessions.iter().map(|(id, s)| (id.clone(), s.view.clone())).collect();
        let focused = views
            .iter()
            .filter(|(_, v)| v.read(cx).focus_handle().is_focused(window))
            .map(|(id, _)| id.clone())
            .collect();
        let pane_folders = state
            .active_of(window_id)
            .filter(|ws| ws.parked)
            .map(|ws| {
                ws.panes
                    .iter()
                    .filter_map(|p| state.pane_folder(&p.id).map(|f| (p.id.clone(), f.to_string_lossy().to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            window_height: f32::from(window.viewport_size().height),
            pane_folders,
            window_id: window_id.to_string(),
            sidebar_collapsed: state.collapsed_of(window_id),
            view: state.view_of(window_id),
            show_parked: state.show_parked,
            workspaces: state.mine_of(window_id).into_iter().cloned().collect(),
            active: state.active_of(window_id).cloned(),
            open_windows: state.open_windows.clone(),
            update: state.update.clone(),
            shows_update: state.open_windows.first().map_or(true, |w| w == window_id),
            views,
            focused,
            menu: if state.menu_owner == window_id { state.menu.clone() } else { None },
            settings: state.data.settings.clone(),
            samples: state.resources.read(cx).samples.iter().copied().collect(),
            network_unit: state.data.settings.resources.network_unit,
            settings_section: state.settings_section,
            confirm_reset: state.confirm_reset,
            setting_inputs,
            editor,
            pane_editor,
        }
    }
}

/// A shortcut as it is written on this system: ⌘B on a Mac, Ctrl+Shift+B elsewhere.
pub fn shortcut(key: &str) -> String {
    if MAC { format!("\u{2318}{key}") } else { format!("Ctrl+Shift+{key}") }
}

// ---- The root view -------------------------------------------------------------------------

pub struct Root {
    state: Entity<AppState>,
    window_id: String,
    focus_subs: HashMap<String, Subscription>,
    editor: Option<(Edit, Entity<TextInput>, Subscription)>,
    pane_editor: Option<(PaneFields, [Subscription; 2])>,
    setting_inputs: HashMap<&'static str, (Entity<TextInput>, Subscription)>,
    /// The window title last set, so it is only set again when it changes.
    last_title: String,
    _observe: Subscription,
    _feed: Subscription,
}

impl Root {
    pub fn new(state: Entity<AppState>, window_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _observe = cx.observe(&state, |_, _, cx| cx.notify());
        let feed = state.read(cx).resources.clone();
        let _feed = cx.observe(&feed, |_, _, cx| cx.notify());
        // Whichever window is in front is the one the menus and shortcuts act on.
        let id = window_id.clone();
        cx.observe_window_activation(window, move |this, window, cx| {
            if window.is_window_active() {
                this.mark_current(cx);
            }
        })
        .detach();
        let _ = id;
        // Where the window is, so the next launch puts it back.
        cx.observe_window_bounds(window, |this, window, cx| {
            remember_bounds(&this.state, false, &this.window_id, window, cx);
        })
        .detach();
        // The title follows the terminal that has the focus (what its program sets).
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update_in(cx, |this, window, cx| this.sync_title(window, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            state,
            window_id,
            focus_subs: HashMap::new(),
            editor: None,
            pane_editor: None,
            setting_inputs: HashMap::new(),
            last_title: String::new(),
            _observe,
            _feed,
        }
    }

    /// The window title: the workspace, and the title the program in its focused terminal set.
    fn sync_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = {
            let state = self.state.read(cx);
            match state.active_of(&self.window_id) {
                Some(ws) => {
                    let program = ws
                        .active_pane_id
                        .as_ref()
                        .and_then(|id| state.sessions.get(id))
                        .map(|s| s.view.read(cx).title().trim().to_string())
                        .unwrap_or_default();
                    if program.is_empty() { ws.name.clone() } else { format!("{} \u{b7} {}", ws.name, program) }
                }
                None => "sdterm".to_string(),
            }
        };
        if title != self.last_title {
            window.set_window_title(&title);
            self.last_title = title;
        }
    }

    /// Makes this window the one actions apply to.
    fn mark_current(&self, cx: &mut Context<Self>) {
        let id = self.window_id.clone();
        self.state.update(cx, |s, _| s.window_id = id);
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
        let me = self.window_id.clone();
        let pending = self.state.update(cx, |state, _| match state.pending_focus.clone() {
            Some(id) if state.pane_window(&id).as_deref() == Some(me.as_str()) => state.pending_focus.take(),
            _ => None,
        });
        if let Some(id) = pending {
            if let Some((_, handle)) = handles.iter().find(|(h, _)| *h == id) {
                handle.focus(window);
            }
        }
    }

    /// Creates the text field when a workspace starts being renamed, and drops it when it ends.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = {
            let s = self.state.read(cx);
            match &s.editing {
                Some(edit @ Edit::WorkspaceName(_)) if s.edit_owner == self.window_id => Some(edit.clone()),
                _ => None,
            }
        };
        if self.editor.as_ref().map(|(e, _, _)| e) == wanted.as_ref() {
            return;
        }
        self.editor = wanted.map(|edit| {
            let initial = match &edit {
                Edit::WorkspaceName(id) => self.state.read(cx).ws(id).map(|w| w.name.clone()).unwrap_or_default(),
                Edit::PaneDetails(_) => String::new(),
            };
            let input = cx.new(|cx| TextInput::new(&initial, "Name", false, true, window, cx));
            input.read(cx).focus(window);
            let sub = cx.subscribe_in(&input, window, |this, _input, event: &InputEvent, _window, cx| {
                this.on_input(event, cx);
            });
            (edit, input, sub)
        });
    }

    /// Creates the name and note fields while a terminal's details are open, and drops them after.
    fn sync_pane_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = {
            let s = self.state.read(cx);
            match &s.editing {
                Some(Edit::PaneDetails(id)) if s.edit_owner == self.window_id => Some(id.clone()),
                _ => None,
            }
        };
        if self.pane_editor.as_ref().map(|(f, _)| &f.id) == wanted.as_ref() {
            return;
        }
        self.pane_editor = wanted.and_then(|id| {
            let (name, note, position) = {
                let state = self.state.read(cx);
                let (ws, i) = state.find_pane(&id)?;
                (ws.panes[i].name.clone(), ws.panes[i].note.clone(), i + 1)
            };
            let name_input =
                cx.new(|cx| TextInput::new(&name, &format!("Terminal {position}"), false, true, window, cx));
            let note_input = cx.new(|cx| {
                TextInput::new(&note, "What are you working on? Where did you leave off?", true, false, window, cx)
                    .with_rows(4)
            });
            name_input.read(cx).focus(window);
            let subscribe = |input: &Entity<TextInput>, is_name: bool, cx: &mut Context<Self>| {
                let id = id.clone();
                cx.subscribe_in(input, window, move |this, _input, event: &InputEvent, window, cx| {
                    this.on_pane_field(&id, is_name, event, window, cx);
                })
            };
            let subs = [subscribe(&name_input, true, cx), subscribe(&note_input, false, cx)];
            Some((PaneFields { id: id.clone(), name: name_input, note: note_input }, subs))
        });
    }

    /// What typing in the name or the note of a terminal does: it is kept as it is written.
    fn on_pane_field(
        &mut self,
        pane: &str,
        is_name: bool,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Changed(text) => self.state.update(cx, |s, cx| {
                if is_name {
                    s.rename_pane(pane, text, cx)
                } else {
                    s.set_pane_note(pane, text, cx)
                }
            }),
            InputEvent::Submit(_) | InputEvent::Cancel => self.state.update(cx, |s, cx| s.end_edit(true, cx)),
            InputEvent::Tab => {
                if let Some((fields, _)) = &self.pane_editor {
                    let next = if is_name { &fields.note } else { &fields.name };
                    next.read(cx).focus(window);
                }
            }
            // Moving between the two fields is not the end of the edit.
            InputEvent::Blur(_) => {}
        }
    }

    /// The text fields of the Terminal settings exist while that page is open.
    fn sync_setting_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        const FIELDS: [(&str, &str); 3] = [
            ("shell", "Login shell"),
            ("start_directory", "Home folder"),
            ("custom_font", "Font name"),
        ];
        let (view, section, settings) = {
            let s = self.state.read(cx);
            (s.view_of(&self.window_id), s.settings_section, s.data.settings.clone())
        };
        if view != View::Settings || section != SettingsSection::Terminal {
            self.setting_inputs.clear();
            return;
        }
        let value = |key: &str| match key {
            "shell" => settings.terminal.shell.clone(),
            "start_directory" => settings.terminal.start_directory.clone(),
            _ => settings.terminal.custom_font_family.clone(),
        };
        for (key, placeholder) in FIELDS {
            // A value changed from elsewhere (the folder chooser): start the field again.
            let stale = self
                .setting_inputs
                .get(key)
                .is_some_and(|(input, _)| {
                    let input = input.read(cx);
                    input.text() != value(key) && !input.is_focused(window)
                });
            if stale {
                self.setting_inputs.remove(key);
            }
            if self.setting_inputs.contains_key(key) {
                continue;
            }
            let initial = value(key);
            let input = cx.new(|cx| TextInput::new(&initial, placeholder, false, false, window, cx));
            let sub = cx.subscribe_in(&input, window, move |this, _input, event: &InputEvent, _window, cx| {
                let text = match event {
                    InputEvent::Submit(t) | InputEvent::Blur(t) => t.clone(),
                    InputEvent::Cancel | InputEvent::Changed(_) | InputEvent::Tab => return,
                };
                this.state.update(cx, |state, cx| {
                    state.update_settings(cx, |s| match key {
                        "shell" => s.terminal.shell = text.trim().to_string(),
                        "start_directory" => s.terminal.start_directory = text.trim().to_string(),
                        _ => s.terminal.custom_font_family = text.trim().to_string(),
                    })
                });
            });
            self.setting_inputs.insert(key, (input, sub));
        }
    }

    fn on_input(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
        let (text, refocus) = match event {
            InputEvent::Submit(text) => (Some(text.clone()), true),
            InputEvent::Blur(text) => (Some(text.clone()), false),
            InputEvent::Cancel => (None, true),
            InputEvent::Changed(_) | InputEvent::Tab => return,
        };
        let Some((edit, _input, _sub)) = self.editor.take() else { return };
        self.state.update(cx, |state, cx| {
            if let (Some(text), Edit::WorkspaceName(id)) = (text, &edit) {
                state.rename_workspace(id, &text, cx);
            }
            state.end_edit(refocus, cx);
        });
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_focus(window, cx);
        self.sync_title(window, cx);
        self.sync_editor(window, cx);
        self.sync_pane_editor(window, cx);
        self.sync_setting_inputs(window, cx);
        let theme = cx.global::<Theme>().clone();
        let editor = self.editor.as_ref().map(|(e, i, _)| (e.clone(), i.clone()));
        let pane_editor = self.pane_editor.as_ref().map(|(f, _)| f.clone());
        let inputs = self.setting_inputs.iter().map(|(k, (i, _))| (*k, i.clone())).collect();
        let snap = Snapshot::take(self.state.read(cx), &self.window_id, editor, pane_editor, inputs, window, cx);
        let state = self.state.clone();

        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.mark_current(cx)))
            .on_action(cx.listener(|this, _: &NewTerminal, _, cx| {
                this.mark_current(cx);
                this.state.update(cx, |s, cx| s.add_pane(cx))
            }))
            .on_action(cx.listener(|this, _: &NewWorkspace, _, cx| {
                this.mark_current(cx);
                this.state.update(cx, |s, cx| s.add_workspace(cx))
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.mark_current(cx);
                this.state.update(cx, |s, cx| s.toggle_sidebar(cx))
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| {
                this.mark_current(cx);
                this.state.update(cx, |s, cx| {
                    let window = s.window_id.clone();
                    let next = if s.view_of(&window) == View::Settings { View::Terminals } else { View::Settings };
                    s.set_view(next, cx)
                })
            }))
            .on_action(cx.listener(|this, _: &CloseTerminal, _, cx| {
                this.mark_current(cx);
                this.state.update(cx, |s, cx| {
                    let pane = s.active().and_then(|w| w.active_pane_id.clone());
                    if let Some(pane) = pane {
                        s.close_pane(&pane, cx);
                    }
                })
            }));

        if let Some(backdrop) = sidebar::menu_backdrop(&snap, &state) {
            root = root.child(backdrop);
        }
        if !snap.sidebar_collapsed {
            root = root.child(sidebar::sidebar(&snap, &theme, &state));
        }
        root.child(panes::main_area(snap, &theme, &state))
    }
}

/// Opens an app window showing the workspaces assigned to `window_id`.
pub fn open_app_window(cx: &mut App, state: &Entity<AppState>, window_id: String) {
    use gpui::{AppContext, Bounds, TitlebarOptions, WindowOptions, point, size};
    state.update(cx, |s, _| s.register_window(&window_id));
    let saved = state.read(cx).data.window_bounds.get(&window_id).copied();
    let default = Bounds::centered(None, size(px(1280.), px(800.)), cx);
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("sdterm".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.), px(14.))),
        }),
        window_bounds: Some(opening_bounds(saved, default, cx)),
        ..Default::default()
    };
    let (root_state, id) = (state.clone(), window_id.clone());
    let opened = cx.open_window(options, move |window, cx| {
        let closing_state = root_state.clone();
        let closing_id = id.clone();
        window.on_window_should_close(cx, move |_window, cx| {
            // The workspaces of a closed window carry on in another one; the last window ends the app.
            let any_left = closing_state.update(cx, |s, cx| s.window_closed(&closing_id, cx));
            if !any_left {
                cx.quit();
            }
            true
        });
        cx.new(|cx| Root::new(root_state, id, window, cx))
    });
    if opened.is_err() {
        eprintln!("could not open the window {window_id}");
    }
}
