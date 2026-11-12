//! The state of the app: workspaces, terminals, which shell is running where.
//!
//! One shared object that every view reads. Every change goes through a method here, which keeps
//! the shells in step with the data (park stops them, launch starts them, closing a terminal ends
//! its shell) and schedules a save.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{AnyWindowHandle, AppContext, Context, Entity, Task};

use crate::metrics::WidgetKind;
use crate::resources::ResourceFeed;
use crate::model::{
    LayoutId, PANE_COLORS, Pane, PaneColor, PersistedWorkspaces, Settings, StoreData, Workspace, new_id,
};
use crate::session::{self, Session};
use crate::updater::{self, Offer, UpdateStatus};
use crate::store;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Terminal,
    Workspaces,
    Resources,
    Updates,
    Shortcuts,
    About,
}

pub const SETTINGS_SECTIONS: [(SettingsSection, &str); 7] = [
    (SettingsSection::Appearance, "Appearance"),
    (SettingsSection::Terminal, "Terminal"),
    (SettingsSection::Workspaces, "Workspaces"),
    (SettingsSection::Resources, "Resources"),
    (SettingsSection::Updates, "Updates"),
    (SettingsSection::Shortcuts, "Shortcuts"),
    (SettingsSection::About, "About"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Terminals,
    Settings,
}

/// Something being renamed or written in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    WorkspaceName(String),
    PaneName(String),
    PaneNote(String),
}

/// A small menu that is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Menu {
    Layout,
    Workspace(String),
    ConfirmDelete(String),
    PaneColor(String),
}

pub struct AppState {
    pub data: StoreData,
    /// The window the user is working in: "main", "w2", ... Actions apply to it.
    pub window_id: String,
    /// The app windows that are open.
    pub open_windows: Vec<String>,
    /// Set when the app is quitting, so closing the windows does not shuffle workspaces around.
    pub quitting: bool,
    /// Which window opened the menu or the editor that is open.
    pub menu_owner: String,
    pub edit_owner: String,
    /// The running shells, by terminal id.
    pub sessions: HashMap<String, Session>,
    /// What each window shows.
    pub views: HashMap<String, View>,
    /// The windows whose sidebar is hidden.
    pub collapsed: HashSet<String>,
    pub resources: Entity<ResourceFeed>,
    /// The pop-out windows that are open.
    pub widgets: HashMap<WidgetKind, AnyWindowHandle>,
    pub settings_section: SettingsSection,
    pub show_parked: bool,
    pub update: UpdateStatus,
    offer: Option<Offer>,
    pub confirm_reset: bool,
    pub editing: Option<Edit>,
    pub menu: Option<Menu>,
    /// A terminal that should get the keyboard focus on the next frame.
    pub pending_focus: Option<String>,
    /// What happened while loading the data file, for the log.
    pub load_note: String,
    save_task: Option<Task<()>>,
    _cwd_task: Option<Task<()>>,
}

/// The name shown for a terminal: the one the user gave it, or its position.
pub fn pane_label(pane: &Pane, index: usize) -> String {
    if pane.name.trim().is_empty() {
        format!("Terminal {}", index + 1)
    } else {
        pane.name.clone()
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn is_dir(path: &str) -> bool {
    !path.is_empty() && Path::new(path).is_dir()
}

#[cfg(target_os = "macos")]
fn cwd_of(pid: u32) -> Option<PathBuf> {
    let out = std::process::Command::new("lsof")
        .args(["-a", "-d", "cwd", "-Fn", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix('n').map(PathBuf::from))
}

#[cfg(target_os = "linux")]
fn cwd_of(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cwd_of(_pid: u32) -> Option<PathBuf> {
    None // Windows: shells report their folder with OSC 7 / OSC 9;9 (later)
}

impl AppState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let loaded = store::load();
        let mut data = loaded.data;
        let window_id = "main".to_string();

        // First launch, or an empty file: start with one workspace with one terminal.
        let empty = data.workspaces.as_ref().is_none_or(|w| w.workspaces.is_empty());
        if empty {
            let mut ws = Workspace::new("Workspace 1", &window_id);
            ws.panes.push(Pane {
                id: new_id("p"),
                name: String::new(),
                note: String::new(),
                color: Some(PANE_COLORS[0]),
            });
            ws.active_pane_id = Some(ws.panes[0].id.clone());
            data.workspaces = Some(PersistedWorkspaces {
                active_by_window: HashMap::from([(window_id.clone(), ws.id.clone())]),
                workspaces: vec![ws],
            });
        }

        let resources = cx.new(|cx| ResourceFeed::new(data.settings.resources.interval_ms, cx));
        let mut state = Self {
            data,
            window_id,
            open_windows: vec!["main".to_string()],
            quitting: false,
            menu_owner: String::new(),
            edit_owner: String::new(),
            sessions: HashMap::new(),
            views: HashMap::new(),
            collapsed: HashSet::new(),
            resources,
            widgets: HashMap::new(),
            settings_section: SettingsSection::Appearance,
            show_parked: false,
            update: UpdateStatus::Idle,
            offer: None,
            confirm_reset: false,
            editing: None,
            menu: None,
            pending_focus: None,
            load_note: loaded.note,
            save_task: None,
            _cwd_task: None,
        };
        state.ensure_active_exists();
        state.ensure_sessions(cx);
        if let Some(id) = state.active().and_then(|w| w.active_pane_id.clone()) {
            state.pending_focus = Some(id);
        }
        state.start_cwd_tracking(cx);
        state
    }

    // ---- Reading ------------------------------------------------------------------------

    fn all(&self) -> &[Workspace] {
        self.data.workspaces.as_ref().map_or(&[], |w| w.workspaces.as_slice())
    }

    fn all_mut(&mut self) -> &mut Vec<Workspace> {
        &mut self.data.workspaces.get_or_insert_with(Default::default).workspaces
    }

    fn ws_mut(&mut self, id: &str) -> Option<&mut Workspace> {
        self.all_mut().iter_mut().find(|w| w.id == id)
    }

    pub fn ws(&self, id: &str) -> Option<&Workspace> {
        self.all().iter().find(|w| w.id == id)
    }

    /// The workspaces of a window.
    pub fn mine_of(&self, window: &str) -> Vec<&Workspace> {
        self.all().iter().filter(|w| w.window_id == window).collect()
    }

    /// The workspaces of the window the user is working in.
    pub fn mine(&self) -> Vec<&Workspace> {
        self.mine_of(&self.window_id)
    }

    pub fn view_of(&self, window: &str) -> View {
        self.views.get(window).copied().unwrap_or(View::Terminals)
    }

    pub fn collapsed_of(&self, window: &str) -> bool {
        self.collapsed.contains(window)
    }

    /// The window a terminal is shown in.
    pub fn pane_window(&self, pane_id: &str) -> Option<String> {
        self.find_pane(pane_id).map(|(w, _)| w.window_id.clone())
    }

    pub fn active_id_of(&self, window: &str) -> Option<String> {
        let picked = self
            .data
            .workspaces
            .as_ref()
            .and_then(|w| w.active_by_window.get(window))
            .filter(|id| self.mine_of(window).iter().any(|w| &w.id == *id))
            .cloned();
        picked.or_else(|| self.mine_of(window).first().map(|w| w.id.clone()))
    }

    pub fn active_of(&self, window: &str) -> Option<&Workspace> {
        let id = self.active_id_of(window)?;
        self.ws(&id)
    }

    pub fn active_id(&self) -> Option<String> {
        self.active_id_of(&self.window_id)
    }

    pub fn active(&self) -> Option<&Workspace> {
        self.active_of(&self.window_id)
    }

    fn ensure_active_exists(&mut self) {
        let windows: Vec<String> = self.all().iter().map(|w| w.window_id.clone()).collect::<HashSet<_>>().into_iter().collect();
        for window in windows {
            if let Some(id) = self.active_id_of(&window) {
                self.data
                    .workspaces
                    .get_or_insert_with(Default::default)
                    .active_by_window
                    .insert(window, id);
            }
        }
    }

    /// A terminal and the workspace it is in.
    pub fn find_pane(&self, pane_id: &str) -> Option<(&Workspace, usize)> {
        self.all().iter().find_map(|w| {
            w.panes.iter().position(|p| p.id == pane_id).map(|i| (w, i))
        })
    }

    // ---- Shells -------------------------------------------------------------------------

    fn start_dir(&self, ws: &Workspace, pane_id: &str) -> PathBuf {
        let remembered = self.data.pane_cwds.get(pane_id).map(String::as_str).unwrap_or("");
        for candidate in [remembered, ws.folder.as_str(), self.data.settings.terminal.start_directory.as_str()] {
            if is_dir(candidate) {
                return PathBuf::from(candidate);
            }
        }
        home()
    }

    /// Starts a shell for every terminal of a running workspace of this window that has none.
    pub fn ensure_sessions(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<(String, PathBuf)> = self
            .all()
            .iter()
            .filter(|w| !w.parked)
            .flat_map(|w| w.panes.iter().map(move |p| (w, p)))
            .filter(|(_, p)| !self.sessions.contains_key(&p.id))
            .map(|(w, p)| (p.id.clone(), self.start_dir(w, &p.id)))
            .collect();

        let theme = cx.global::<Theme>().clone();
        let settings = self.data.settings.terminal.clone();
        for (pane_id, cwd) in wanted {
            let me = cx.weak_entity();
            let id = pane_id.clone();
            let started = session::spawn(cx, &settings, &theme, &cwd, move |cx| {
                me.update(cx, |state, cx| state.shell_exited(&id, cx)).ok();
            });
            match started {
                Ok(session) => {
                    self.sessions.insert(pane_id, session);
                }
                Err(err) => eprintln!("could not start a shell for {pane_id}: {err:#}"),
            }
        }
    }

    fn end_session(&mut self, pane_id: &str) {
        if let Some(mut session) = self.sessions.remove(pane_id) {
            session.kill();
        }
    }

    /// Remembers where each shell is, so a terminal starts again in the same folder.
    fn start_cwd_tracking(&mut self, cx: &mut Context<Self>) {
        self._cwd_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let Ok(pids) = this.update(cx, |state, _| state.pids()) else { break };
                let found = cx
                    .background_executor()
                    .spawn(async move {
                        pids.into_iter()
                            .filter_map(|(id, pid)| cwd_of(pid).map(|dir| (id, dir)))
                            .collect::<Vec<_>>()
                    })
                    .await;
                let updated = this.update(cx, |state, cx| {
                    let mut changed = false;
                    for (id, dir) in found {
                        let dir = dir.to_string_lossy().to_string();
                        if state.data.pane_cwds.get(&id) != Some(&dir) {
                            state.data.pane_cwds.insert(id, dir);
                            changed = true;
                        }
                    }
                    if changed {
                        state.schedule_save(cx);
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    fn pids(&self) -> Vec<(String, u32)> {
        self.sessions.iter().filter_map(|(id, s)| s.pid().map(|pid| (id.clone(), pid))).collect()
    }

    fn snapshot_cwds(&mut self) {
        for (id, pid) in self.pids() {
            if let Some(dir) = cwd_of(pid) {
                self.data.pane_cwds.insert(id, dir.to_string_lossy().to_string());
            }
        }
    }

    // ---- Saving -------------------------------------------------------------------------

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        self.schedule_save(cx);
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            this.update(cx, |state, _| state.save_now()).ok();
        }));
    }

    pub fn save_now(&self) {
        if let Err(err) = store::save(&self.data) {
            eprintln!("could not save: {err}");
        }
    }

    /// Before quitting: remember the folders, stop every shell, write the file.
    pub fn shutdown(&mut self) {
        self.snapshot_cwds();
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in ids {
            self.end_session(&id);
        }
        self.save_now();
    }

    // ---- Workspaces ---------------------------------------------------------------------

    pub fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.views.insert(self.window_id.clone(), view);
        cx.notify();
    }

    pub fn set_settings_section(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        self.settings_section = section;
        cx.notify();
    }

    /// Changes a setting. It takes effect at once: the theme, and the font, colors and keys of the
    /// terminals that are already open.
    pub fn update_settings(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Settings)) {
        let mut settings = self.data.settings.clone();
        change(&mut settings);
        self.data.settings = settings.normalized();
        self.apply_settings(cx);
        self.changed(cx);
    }

    pub fn reset_settings(&mut self, cx: &mut Context<Self>) {
        self.data.settings = Settings::default();
        self.apply_settings(cx);
        self.changed(cx);
    }

    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        let dark = matches!(
            cx.window_appearance(),
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark
        );
        let theme = Theme::for_mode(self.data.settings.appearance.theme, dark);
        cx.set_global(theme.clone());
        let interval = self.data.settings.resources.interval_ms;
        self.resources.update(cx, |feed, _| feed.interval_ms = interval);
        let settings = self.data.settings.terminal.clone();
        for session in self.sessions.values() {
            let config = session::terminal_config(&settings, &theme);
            session.view.update(cx, |view, cx| {
                view.update_config(config, cx);
                view.set_option_as_meta(settings.option_as_meta);
                view.set_copy_on_select(settings.copy_on_select);
            });
        }
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&self.window_id) {
            self.collapsed.insert(self.window_id.clone());
        }
        cx.notify();
    }

    pub fn begin_edit(&mut self, edit: Edit, cx: &mut Context<Self>) {
        self.menu = None;
        self.edit_owner = self.window_id.clone();
        self.editing = Some(edit);
        cx.notify();
    }

    /// Ends an edit. `refocus` hands the keyboard back to the terminal (after Enter or Escape, not
    /// when the user clicked somewhere else on purpose).
    pub fn end_edit(&mut self, refocus: bool, cx: &mut Context<Self>) {
        self.editing = None;
        if refocus {
            if let Some(pane) = self.active().and_then(|w| w.active_pane_id.clone()) {
                self.pending_focus = Some(pane);
            }
        }
        cx.notify();
    }

    pub fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.editing = None;
        self.menu_owner = self.window_id.clone();
        self.menu = if self.menu.as_ref() == Some(&menu) { None } else { Some(menu) };
        cx.notify();
    }

    pub fn open_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.menu_owner = self.window_id.clone();
        self.menu = Some(menu);
        cx.notify();
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// The folder a terminal's shell is in, or its workspace's folder.
    pub fn pane_folder(&self, pane_id: &str) -> Option<PathBuf> {
        let remembered = self.data.pane_cwds.get(pane_id).filter(|d| is_dir(d)).map(PathBuf::from);
        remembered.or_else(|| {
            self.find_pane(pane_id)
                .map(|(w, _)| w.folder.clone())
                .filter(|d| is_dir(d))
                .map(PathBuf::from)
        })
    }

    pub fn toggle_parked_list(&mut self, cx: &mut Context<Self>) {
        self.show_parked = !self.show_parked;
        cx.notify();
    }

    pub fn set_active_workspace(&mut self, id: &str, cx: &mut Context<Self>) {
        let window = self.window_id.clone();
        self.data
            .workspaces
            .get_or_insert_with(Default::default)
            .active_by_window
            .insert(window, id.to_string());
        self.views.insert(self.window_id.clone(), View::Terminals);
        self.menu = None;
        if let Some(pane) = self.ws(id).and_then(|w| w.active_pane_id.clone()) {
            self.pending_focus = Some(pane);
        }
        self.changed(cx);
    }

    pub fn add_workspace(&mut self, cx: &mut Context<Self>) {
        let n = self.all().len() + 1;
        let mut ws = Workspace::new(format!("Workspace {n}"), &self.window_id);
        let pane = self.new_pane_for(&ws);
        ws.active_pane_id = Some(pane.id.clone());
        ws.panes.push(pane);
        let id = ws.id.clone();
        self.all_mut().push(ws);
        self.ensure_sessions(cx);
        self.set_active_workspace(&id, cx);
    }

    pub fn rename_workspace(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(ws) = self.ws_mut(id) {
            ws.name = name.chars().take(60).collect();
            self.changed(cx);
        }
    }

    pub fn delete_workspace(&mut self, id: &str, cx: &mut Context<Self>) {
        let pane_ids: Vec<String> =
            self.ws(id).map(|w| w.panes.iter().map(|p| p.id.clone()).collect()).unwrap_or_default();
        for pane in &pane_ids {
            self.end_session(pane);
            self.data.pane_cwds.remove(pane);
        }
        self.all_mut().retain(|w| w.id != id);
        self.ensure_active_exists();
        self.changed(cx);
    }

    /// Parks a workspace: its shells stop, but names, notes, colors and folders are kept.
    pub fn set_parked(&mut self, id: &str, parked: bool, cx: &mut Context<Self>) {
        if parked {
            self.snapshot_cwds();
            let pane_ids: Vec<String> =
                self.ws(id).map(|w| w.panes.iter().map(|p| p.id.clone()).collect()).unwrap_or_default();
            for pane in &pane_ids {
                self.end_session(pane);
            }
        }
        if let Some(ws) = self.ws_mut(id) {
            ws.parked = parked;
        }
        if !parked {
            self.ensure_sessions(cx);
        }
        self.changed(cx);
    }

    pub fn set_layout(&mut self, id: &str, layout: LayoutId, cx: &mut Context<Self>) {
        if let Some(ws) = self.ws_mut(id) {
            ws.layout = layout;
            ws.split = None;
            self.changed(cx);
        }
    }

    pub fn set_split(&mut self, id: &str, split: f64, cx: &mut Context<Self>) {
        if let Some(ws) = self.ws_mut(id) {
            ws.split = Some(split.clamp(0.2, 0.8));
            self.changed(cx);
        }
    }

    /// Back to the layout's own proportions (double-clicking the divider).
    pub fn reset_split(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(ws) = self.ws_mut(id) {
            ws.split = None;
            self.changed(cx);
        }
    }

    pub fn set_workspace_folder(&mut self, id: &str, folder: &str, cx: &mut Context<Self>) {
        if let Some(ws) = self.ws_mut(id) {
            ws.folder = folder.to_string();
            self.changed(cx);
        }
    }

    /// The windows the saved workspaces belong to, "main" first.
    pub fn window_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.all().iter().map(|w| w.window_id.clone()).collect::<HashSet<_>>().into_iter().collect();
        ids.sort_by_key(|id| (id != "main", id.clone()));
        if ids.is_empty() {
            ids.push("main".to_string());
        }
        ids
    }

    // ---- Updates ------------------------------------------------------------------------

    /// Looks for a new version now. A check you did not ask for stays quiet when it fails.
    pub fn check_for_update(&mut self, manual: bool, cx: &mut Context<Self>) {
        if !updater::enabled() {
            if manual {
                self.update = UpdateStatus::Error("Updates are off in a development build.".into());
                cx.notify();
            }
            return;
        }
        if matches!(self.update, UpdateStatus::Checking | UpdateStatus::Downloading(_)) {
            return;
        }
        self.update = UpdateStatus::Checking;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { updater::check(&updater::feed_base()) })
                .await;
            this.update(cx, |state, cx| {
                match result {
                    Ok(Some(offer)) => {
                        state.update = UpdateStatus::Available {
                            version: offer.version.clone(),
                            notes: offer.notes.clone(),
                        };
                        state.offer = Some(offer);
                    }
                    Ok(None) => state.update = UpdateStatus::UpToDate,
                    Err(err) if manual => state.update = UpdateStatus::Error(format!("{err:#}")),
                    Err(_) => state.update = UpdateStatus::Idle,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Checks at startup and every few hours while the app is open.
    pub fn schedule_update_checks(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(10)).await;
            loop {
                let again = this.update(cx, |state, cx| {
                    if state.data.settings.updates.auto_check {
                        state.check_for_update(false, cx);
                    }
                });
                if again.is_err() {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(6 * 60 * 60)).await;
            }
        })
        .detach();
    }

    /// Downloads the new version, installs it and restarts. On Windows and Linux it opens the
    /// download page instead.
    pub fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(offer) = self.offer.clone() else { return };
        if !cfg!(target_os = "macos") {
            cx.open_url(updater::DOWNLOAD_PAGE);
            return;
        }
        self.update = UpdateStatus::Downloading(0);
        cx.notify();
        let percent = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        // The download runs in the background and reports into a number; this loop shows it.
        let (p, f) = (percent.clone(), finished.clone());
        cx.spawn(async move |this, cx| {
            while !f.load(std::sync::atomic::Ordering::Relaxed) {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                let n = p.load(std::sync::atomic::Ordering::Relaxed);
                if this
                    .update(cx, |state, cx| {
                        if matches!(state.update, UpdateStatus::Downloading(_)) {
                            state.update = UpdateStatus::Downloading(n);
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let (p, f) = (percent, finished);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let zip = updater::download(&offer, |n| p.store(n, std::sync::atomic::Ordering::Relaxed))?;
                    updater::install_mac(&zip)
                })
                .await;
            f.store(true, std::sync::atomic::Ordering::Relaxed);
            this.update(cx, |state, cx| match result {
                Ok(()) => {
                    // The swap script waits for this process to end, then reopens the new app.
                    state.quitting = true;
                    cx.quit();
                }
                Err(err) => {
                    state.update = UpdateStatus::Error(format!("{err:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn dismiss_update(&mut self, cx: &mut Context<Self>) {
        if matches!(self.update, UpdateStatus::Available { .. } | UpdateStatus::Error(_)) {
            self.update = UpdateStatus::Idle;
            cx.notify();
        }
    }

    // ---- Windows ------------------------------------------------------------------------

    /// The next free window id: w2, w3, ...
    pub fn new_window_id(&self) -> String {
        (2..).map(|n| format!("w{n}")).find(|id| !self.open_windows.contains(id)).unwrap()
    }

    pub fn register_window(&mut self, window: &str) {
        if !self.open_windows.iter().any(|w| w == window) {
            self.open_windows.push(window.to_string());
        }
    }

    /// Gives a window its own first workspace, with a terminal in it.
    pub fn add_workspace_in(&mut self, window: &str, cx: &mut Context<Self>) {
        let n = self.all().len() + 1;
        let mut ws = Workspace::new(format!("Workspace {n}"), window);
        let pane = self.new_pane_for(&ws);
        ws.active_pane_id = Some(pane.id.clone());
        ws.panes.push(pane);
        let id = ws.id.clone();
        self.all_mut().push(ws);
        self.ensure_sessions(cx);
        self.data
            .workspaces
            .get_or_insert_with(Default::default)
            .active_by_window
            .insert(window.to_string(), id);
        self.changed(cx);
    }

    /// Moves a workspace, shells and all, to another window.
    pub fn move_workspace(&mut self, id: &str, window: &str, cx: &mut Context<Self>) {
        let Some(from) = self.ws(id).map(|w| w.window_id.clone()) else { return };
        if from == window {
            return;
        }
        if let Some(ws) = self.ws_mut(id) {
            ws.window_id = window.to_string();
        }
        self.menu = None;
        self.ensure_active_exists();
        self.data
            .workspaces
            .get_or_insert_with(Default::default)
            .active_by_window
            .insert(window.to_string(), id.to_string());
        self.pending_focus = self.ws(id).and_then(|w| w.active_pane_id.clone());
        self.changed(cx);
    }

    /// A window was closed: its workspaces keep running in another window. Returns whether any
    /// window is left.
    pub fn window_closed(&mut self, window: &str, cx: &mut Context<Self>) -> bool {
        self.open_windows.retain(|w| w != window);
        self.views.remove(window);
        self.collapsed.remove(window);
        if self.quitting {
            return !self.open_windows.is_empty();
        }
        if let Some(target) = self.open_windows.first().cloned() {
            let moving: Vec<String> =
                self.mine_of(window).iter().map(|w| w.id.clone()).collect();
            for id in moving {
                if let Some(ws) = self.ws_mut(&id) {
                    ws.window_id = target.clone();
                }
            }
            self.ensure_active_exists();
            self.changed(cx);
        }
        !self.open_windows.is_empty()
    }

    // ---- Terminals ----------------------------------------------------------------------

    /// A new terminal: the first color of the palette not used in the workspace yet.
    fn new_pane_for(&self, ws: &Workspace) -> Pane {
        let color = if self.data.settings.workspaces.auto_color {
            let used: HashSet<PaneColor> = ws.panes.iter().filter_map(|p| p.color).collect();
            PANE_COLORS
                .iter()
                .find(|c| !used.contains(c))
                .copied()
                .or(Some(PANE_COLORS[ws.panes.len() % PANE_COLORS.len()]))
        } else {
            None
        };
        Pane { id: new_id("p"), name: String::new(), note: String::new(), color }
    }

    /// Adds a terminal to the workspace you are looking at.
    pub fn add_pane(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.active_id() else { return };
        let Some(ws) = self.ws(&id) else { return };
        if ws.parked {
            return;
        }
        let pane = self.new_pane_for(ws);
        let pane_id = pane.id.clone();
        if let Some(ws) = self.ws_mut(&id) {
            ws.panes.push(pane);
            ws.active_pane_id = Some(pane_id.clone());
        }
        self.ensure_sessions(cx);
        self.pending_focus = Some(pane_id);
        self.changed(cx);
    }

    pub fn close_pane(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.end_session(pane_id);
        self.data.pane_cwds.remove(pane_id);
        for ws in self.all_mut() {
            if ws.panes.iter().any(|p| p.id == pane_id) {
                ws.panes.retain(|p| p.id != pane_id);
                if ws.active_pane_id.as_deref() == Some(pane_id) {
                    ws.active_pane_id = ws.panes.last().map(|p| p.id.clone());
                }
            }
        }
        if let Some(next) = self.active().and_then(|w| w.active_pane_id.clone()) {
            self.pending_focus = Some(next);
        }
        self.changed(cx);
    }

    /// The shell ended by itself (the user typed `exit`): the terminal goes with it.
    pub fn shell_exited(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        if self.sessions.contains_key(pane_id) {
            self.close_pane(pane_id, cx);
        }
    }

    pub fn focus_pane(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let ws_id = self.find_pane(pane_id).map(|(w, _)| w.id.clone());
        if let Some(ws_id) = ws_id {
            if let Some(ws) = self.ws_mut(&ws_id) {
                ws.active_pane_id = Some(pane_id.to_string());
            }
        }
        self.changed(cx);
    }

    /// Selects a terminal from anywhere (the sidebar, a click): shows its workspace, makes it the
    /// active one there and asks for the keyboard focus to move to it.
    pub fn focus_pane_and_select(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some((ws_id, window)) =
            self.find_pane(pane_id).map(|(w, _)| (w.id.clone(), w.window_id.clone()))
        else {
            return;
        };
        self.data
            .workspaces
            .get_or_insert_with(Default::default)
            .active_by_window
            .insert(window.clone(), ws_id.clone());
        if let Some(ws) = self.ws_mut(&ws_id) {
            ws.active_pane_id = Some(pane_id.to_string());
        }
        self.views.insert(window, View::Terminals);
        self.pending_focus = Some(pane_id.to_string());
        self.changed(cx);
    }

    fn pane_mut(&mut self, pane_id: &str) -> Option<&mut Pane> {
        self.all_mut().iter_mut().flat_map(|w| w.panes.iter_mut()).find(|p| p.id == pane_id)
    }

    pub fn rename_pane(&mut self, pane_id: &str, name: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.pane_mut(pane_id) {
            p.name = name.trim().chars().take(60).collect();
            self.changed(cx);
        }
    }

    pub fn set_pane_note(&mut self, pane_id: &str, note: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.pane_mut(pane_id) {
            p.note = note.chars().take(2000).collect();
            self.changed(cx);
        }
    }

    pub fn set_pane_color(&mut self, pane_id: &str, color: Option<PaneColor>, cx: &mut Context<Self>) {
        if let Some(p) = self.pane_mut(pane_id) {
            p.color = color;
            self.changed(cx);
        }
    }

    /// Swaps two terminals' positions in a workspace, like dragging one onto the other.
    pub fn swap_panes(&mut self, a: &str, b: &str, cx: &mut Context<Self>) {
        for ws in self.all_mut() {
            let i = ws.panes.iter().position(|p| p.id == a);
            let j = ws.panes.iter().position(|p| p.id == b);
            if let (Some(i), Some(j)) = (i, j) {
                if i != j {
                    ws.panes.swap(i, j);
                }
                break;
            }
        }
        self.changed(cx);
    }

    /// Moves a terminal, with its shell, to the end of another workspace.
    pub fn move_pane(&mut self, pane_id: &str, to: &str, cx: &mut Context<Self>) {
        let Some((from, _)) = self.find_pane(pane_id) else { return };
        let from = from.id.clone();
        if from == to || self.ws(to).is_none_or(|w| w.parked) {
            return;
        }
        let mut moved = None;
        if let Some(ws) = self.ws_mut(&from) {
            if let Some(i) = ws.panes.iter().position(|p| p.id == pane_id) {
                moved = Some(ws.panes.remove(i));
                if ws.active_pane_id.as_deref() == Some(pane_id) {
                    ws.active_pane_id = ws.panes.first().map(|p| p.id.clone());
                }
            }
        }
        if let Some(pane) = moved {
            if let Some(ws) = self.ws_mut(to) {
                ws.active_pane_id = Some(pane.id.clone());
                ws.panes.push(pane);
            }
        }
        self.changed(cx);
    }
}
