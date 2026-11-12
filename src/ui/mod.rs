//! The window: sidebar, title bar and the grid of terminals.
//!
//! Views only read a snapshot of the state and call methods on it; they never change data
//! themselves, so everything they do goes through the same checks and the same saving.

mod controls;
mod panes;
mod parts;
mod settings;
mod sidebar;

use std::collections::{HashMap, HashSet};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Render, Subscription, Window, actions, div, px,
};
use gpui_terminal::TerminalView;

use crate::model::{Settings, Workspace};
use crate::state::{AppState, Edit, Menu, SettingsSection, View};
use crate::text_input::{InputEvent, TextInput};
use crate::theme::Theme;

actions!(
    sdterm,
    [NewTerminal, CloseTerminal, NewWorkspace, ToggleSidebar, OpenSettings, Quit]
);

pub(crate) const MAC: bool = cfg!(target_os = "macos");
const UI_FONT: &str = ".SystemUIFont";
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

// ---- What the views need from the state ----------------------------------------------------

pub(crate) struct Snapshot {
    pub sidebar_collapsed: bool,
    pub view: View,
    pub show_parked: bool,
    pub workspaces: Vec<Workspace>,
    pub active: Option<Workspace>,
    pub views: HashMap<String, Entity<TerminalView>>,
    pub focused: HashSet<String>,
    pub menu: Option<Menu>,
    pub settings: Settings,
    pub settings_section: SettingsSection,
    pub confirm_reset: bool,
    /// The text fields of the settings page.
    pub setting_inputs: HashMap<&'static str, Entity<TextInput>>,
    /// The text field of whatever is being renamed or written, with what it is for.
    pub editor: Option<(Edit, Entity<TextInput>)>,
}

impl Snapshot {
    fn take(
        state: &AppState,
        editor: Option<(Edit, Entity<TextInput>)>,
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
        Self {
            sidebar_collapsed: state.sidebar_collapsed,
            view: state.view,
            show_parked: state.show_parked,
            workspaces: state.mine().into_iter().cloned().collect(),
            active: state.active().cloned(),
            views,
            focused,
            menu: state.menu.clone(),
            settings: state.data.settings.clone(),
            settings_section: state.settings_section,
            confirm_reset: state.confirm_reset,
            setting_inputs,
            editor,
        }
    }
}

/// The monitors at the bottom of the sidebar (CPU, RAM, disk, network).
pub(crate) fn resources_summary(_theme: &Theme, _snap: &Snapshot) -> AnyElement {
    div().into_any_element()
}

// ---- The root view -------------------------------------------------------------------------

pub struct Root {
    state: Entity<AppState>,
    focus_subs: HashMap<String, Subscription>,
    editor: Option<(Edit, Entity<TextInput>, Subscription)>,
    setting_inputs: HashMap<&'static str, (Entity<TextInput>, Subscription)>,
    _observe: Subscription,
}

impl Root {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let _observe = cx.observe(&state, |_, _, cx| cx.notify());
        Self { state, focus_subs: HashMap::new(), editor: None, setting_inputs: HashMap::new(), _observe }
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

    /// Creates the text field when something starts being edited, and drops it when it ends.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = self.state.read(cx).editing.clone();
        if self.editor.as_ref().map(|(e, _, _)| e) == wanted.as_ref() {
            return;
        }
        self.editor = wanted.map(|edit| {
            let (initial, placeholder, multiline) = {
                let state = self.state.read(cx);
                match &edit {
                    Edit::WorkspaceName(id) => {
                        (state.ws(id).map(|w| w.name.clone()).unwrap_or_default(), "Name", false)
                    }
                    Edit::PaneName(id) => (
                        state.find_pane(id).map(|(w, i)| w.panes[i].name.clone()).unwrap_or_default(),
                        "Name",
                        false,
                    ),
                    Edit::PaneNote(id) => (
                        state.find_pane(id).map(|(w, i)| w.panes[i].note.clone()).unwrap_or_default(),
                        "What is this terminal for?",
                        true,
                    ),
                }
            };
            let input = cx.new(|cx| TextInput::new(&initial, placeholder, multiline, true, window, cx));
            input.read(cx).focus(window);
            let sub = cx.subscribe_in(&input, window, |this, _input, event: &InputEvent, _window, cx| {
                this.on_input(event, cx);
            });
            (edit, input, sub)
        });
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
            (s.view, s.settings_section, s.data.settings.clone())
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
                    InputEvent::Cancel => return,
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
        let Some((edit, _input, _sub)) = self.editor.take() else { return };
        let (text, refocus) = match event {
            InputEvent::Submit(text) => (Some(text.clone()), true),
            InputEvent::Blur(text) => (Some(text.clone()), false),
            InputEvent::Cancel => (None, true),
        };
        self.state.update(cx, |state, cx| {
            if let Some(text) = text {
                match &edit {
                    Edit::WorkspaceName(id) => state.rename_workspace(id, &text, cx),
                    Edit::PaneName(id) => state.rename_pane(id, &text, cx),
                    Edit::PaneNote(id) => state.set_pane_note(id, &text, cx),
                }
            }
            state.end_edit(refocus, cx);
        });
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_focus(window, cx);
        self.sync_editor(window, cx);
        self.sync_setting_inputs(window, cx);
        let theme = cx.global::<Theme>().clone();
        let editor = self.editor.as_ref().map(|(e, i, _)| (e.clone(), i.clone()));
        let inputs = self.setting_inputs.iter().map(|(k, (i, _))| (*k, i.clone())).collect();
        let snap = Snapshot::take(self.state.read(cx), editor, inputs, window, cx);
        let state = self.state.clone();

        let mut root = div()
            .relative()
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

        if let Some(backdrop) = sidebar::menu_backdrop(&snap, &state) {
            root = root.child(backdrop);
        }
        if !snap.sidebar_collapsed {
            root = root.child(sidebar::sidebar(&snap, &theme, &state));
        }
        root.child(panes::main_area(snap, &theme, &state))
    }
}
