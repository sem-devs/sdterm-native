//! The state of the app: workspaces, terminals, which shell is running where.
//!
//! One shared object that every view reads. Every change goes through a method here, which keeps
//! the shells in step with the data (park stops them, launch starts them, closing a terminal ends
//! its shell) and schedules a save.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{Context, Task};

use crate::model::{
    LayoutId, PANE_COLORS, Pane, PaneColor, PersistedWorkspaces, StoreData, Workspace, new_id,
};
use crate::session::{self, Session};
use crate::store;
use crate::theme::Theme;

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
    /// This window's id: "main", "w2", ...
    pub window_id: String,
    /// The running shells, by terminal id.
    pub sessions: HashMap<String, Session>,
    pub view: View,
    pub sidebar_collapsed: bool,
    pub show_parked: bool,
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

        let mut state = Self {
            data,
            window_id,
            sessions: HashMap::new(),
            view: View::Terminals,
            sidebar_collapsed: false,
            show_parked: false,
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

    /// The workspaces shown in this window.
    pub fn mine(&self) -> Vec<&Workspace> {
        self.all().iter().filter(|w| w.window_id == self.window_id).collect()
    }

    pub fn active_id(&self) -> Option<String> {
        let picked = self
            .data
            .workspaces
            .as_ref()
            .and_then(|w| w.active_by_window.get(&self.window_id))
            .filter(|id| self.mine().iter().any(|w| &w.id == *id))
            .cloned();
        picked.or_else(|| self.mine().first().map(|w| w.id.clone()))
    }

    pub fn active(&self) -> Option<&Workspace> {
        let id = self.active_id()?;
        self.ws(&id)
    }

    fn ensure_active_exists(&mut self) {
        if let Some(id) = self.active_id() {
            let window = self.window_id.clone();
            self.data
                .workspaces
                .get_or_insert_with(Default::default)
                .active_by_window
                .insert(window, id);
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
            .mine()
            .into_iter()
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
        self.view = view;
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    pub fn begin_edit(&mut self, edit: Edit, cx: &mut Context<Self>) {
        self.menu = None;
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
        self.menu = if self.menu.as_ref() == Some(&menu) { None } else { Some(menu) };
        cx.notify();
    }

    pub fn open_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
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
        self.view = View::Terminals;
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
        let Some(ws_id) = self.find_pane(pane_id).map(|(w, _)| w.id.clone()) else { return };
        let window = self.window_id.clone();
        self.data
            .workspaces
            .get_or_insert_with(Default::default)
            .active_by_window
            .insert(window, ws_id.clone());
        if let Some(ws) = self.ws_mut(&ws_id) {
            ws.active_pane_id = Some(pane_id.to_string());
        }
        self.view = View::Terminals;
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
