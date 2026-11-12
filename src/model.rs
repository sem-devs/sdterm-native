//! The data of the app: workspaces, terminals and settings.
//!
//! The JSON layout is the one the Electron app writes (`sdterm.json`), so the native app can read a
//! file made by it and nobody loses their workspaces, names, notes or colors. Anything unknown or
//! invalid in the file falls back to a default instead of failing to load.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

// ---- Terminals and workspaces -------------------------------------------------------------

/// A label color for a terminal. These are the user's own labels, not UI accents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneColor {
    Cyan,
    Blue,
    Green,
    Amber,
    Orange,
    Rose,
    Violet,
    Graphite,
}

pub const PANE_COLORS: [PaneColor; 8] = [
    PaneColor::Cyan,
    PaneColor::Blue,
    PaneColor::Green,
    PaneColor::Amber,
    PaneColor::Orange,
    PaneColor::Rose,
    PaneColor::Violet,
    PaneColor::Graphite,
];

impl PaneColor {
    pub fn label(self) -> &'static str {
        match self {
            PaneColor::Cyan => "Cyan",
            PaneColor::Blue => "Blue",
            PaneColor::Green => "Green",
            PaneColor::Amber => "Amber",
            PaneColor::Orange => "Orange",
            PaneColor::Rose => "Rose",
            PaneColor::Violet => "Violet",
            PaneColor::Graphite => "Graphite",
        }
    }
}

/// A color that is not one of ours (a hand edit, a newer version) becomes "no color".
fn lenient_color<'de, D: Deserializer<'de>>(d: D) -> Result<Option<PaneColor>, D::Error> {
    let raw = Option::<String>::deserialize(d)?;
    Ok(raw.and_then(|s| {
        serde_json::from_value::<PaneColor>(serde_json::Value::String(s)).ok()
    }))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default, deserialize_with = "lenient_color")]
    pub color: Option<PaneColor>,
}

/// How a workspace arranges its terminals. `Auto` also stands for anything unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutId {
    Columns,
    Rows,
    Grid,
    Quarters,
    MainLeft,
    MainRight,
    MainTop,
    #[serde(other)]
    Auto,
}

pub const LAYOUTS: [LayoutId; 8] = [
    LayoutId::Auto,
    LayoutId::Columns,
    LayoutId::Rows,
    LayoutId::Grid,
    LayoutId::Quarters,
    LayoutId::MainLeft,
    LayoutId::MainRight,
    LayoutId::MainTop,
];

impl LayoutId {
    pub fn label(self) -> &'static str {
        match self {
            LayoutId::Auto => "Auto",
            LayoutId::Columns => "Columns",
            LayoutId::Rows => "Rows",
            LayoutId::Grid => "Grid",
            LayoutId::Quarters => "Quarters",
            LayoutId::MainLeft => "Main on the left",
            LayoutId::MainRight => "Main on the right",
            LayoutId::MainTop => "Main on top",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// Which app window shows it: "main", "w2", "w3", ...
    #[serde(default = "main_window")]
    pub window_id: String,
    /// Parked: its shells are stopped, but names, notes, colors and folders are kept.
    #[serde(default)]
    pub parked: bool,
    /// The project folder; new terminals in this workspace start here.
    #[serde(default)]
    pub folder: String,
    #[serde(default = "auto_layout")]
    pub layout: LayoutId,
    /// Size of the main area (or the first of two) as a fraction, or none for the layout's default.
    #[serde(default)]
    pub split: Option<f64>,
    #[serde(default)]
    pub panes: Vec<Pane>,
    #[serde(default)]
    pub active_pane_id: Option<String>,
}

fn main_window() -> String {
    "main".to_string()
}

fn auto_layout() -> LayoutId {
    LayoutId::Auto
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedWorkspaces {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    /// The selected workspace in each window.
    #[serde(default)]
    pub active_by_window: HashMap<String, String>,
}

// ---- Settings -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    Bar,
    Block,
    Underline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontChoice {
    GeistMono,
    System,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkUnit {
    Bytes,
    Bits,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub theme: ThemeMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowSettings {
    /// Open as large as the screen, as a normal resizable window (not full-screen mode).
    pub start_maximized: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TerminalSettings {
    pub font: FontChoice,
    pub custom_font_family: String,
    pub font_size: f64,
    pub line_height: f64,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub scrollback: u32,
    /// Empty means the platform default (login shell on macOS and Linux, PowerShell on Windows).
    pub shell: String,
    /// Empty means the home directory.
    pub start_directory: String,
    /// Off, like Terminal.app: Option types characters (@ # [ ] on Italian and many other layouts).
    pub option_as_meta: bool,
    pub copy_on_select: bool,
    /// Grey suggestions from earlier commands and smarter Tab completion (zsh).
    pub suggestions: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceSettings {
    pub restore_on_launch: bool,
    pub auto_color: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateSettings {
    /// Look for a new version at startup and every few hours.
    pub auto_check: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SidebarMetrics {
    pub cpu: bool,
    pub ram: bool,
    pub disk: bool,
    pub net: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ResourceSettings {
    pub interval_ms: u32,
    pub network_unit: NetworkUnit,
    pub popout_always_on_top: bool,
    pub show_in_sidebar: SidebarMetrics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Bumped when a default changes in a way existing files must pick up.
    pub version: u32,
    pub appearance: AppearanceSettings,
    pub window: WindowSettings,
    pub terminal: TerminalSettings,
    pub workspaces: WorkspaceSettings,
    pub updates: UpdateSettings,
    pub resources: ResourceSettings,
}

pub const SETTINGS_VERSION: u32 = 2;

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self { theme: ThemeMode::System }
    }
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { start_maximized: true }
    }
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font: FontChoice::GeistMono,
            custom_font_family: String::new(),
            font_size: 13.0,
            line_height: 1.25,
            cursor_style: CursorStyle::Block,
            cursor_blink: false,
            scrollback: 10_000,
            shell: String::new(),
            start_directory: String::new(),
            option_as_meta: false,
            copy_on_select: false,
            suggestions: true,
        }
    }
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self { restore_on_launch: true, auto_color: true }
    }
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_check: true }
    }
}

impl Default for SidebarMetrics {
    fn default() -> Self {
        Self { cpu: true, ram: true, disk: true, net: true }
    }
}

impl Default for ResourceSettings {
    fn default() -> Self {
        Self {
            interval_ms: 2000,
            network_unit: NetworkUnit::Bytes,
            popout_always_on_top: true,
            show_in_sidebar: SidebarMetrics::default(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            appearance: AppearanceSettings::default(),
            window: WindowSettings::default(),
            terminal: TerminalSettings::default(),
            workspaces: WorkspaceSettings::default(),
            updates: UpdateSettings::default(),
            resources: ResourceSettings::default(),
        }
    }
}

impl Settings {
    /// Brings values from an older or hand edited file back into range.
    pub fn normalized(mut self) -> Self {
        if self.version < 2 {
            // v2: Option no longer acts as Meta by default (it broke typing @ and # on Italian).
            self.terminal.option_as_meta = false;
        }
        self.version = SETTINGS_VERSION;
        let t = &mut self.terminal;
        t.font_size = t.font_size.clamp(9.0, 32.0);
        t.line_height = t.line_height.clamp(1.0, 2.0);
        t.scrollback = t.scrollback.clamp(500, 200_000);
        t.custom_font_family.truncate(200);
        t.shell.truncate(500);
        t.start_directory.truncate(1000);
        if ![1000, 2000, 5000].contains(&self.resources.interval_ms) {
            self.resources.interval_ms = 2000;
        }
        self
    }
}

// ---- The file -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The window was maximized (the other numbers are the size to go back to).
    #[serde(default)]
    pub maximized: bool,
}

impl WindowBounds {
    /// Whether enough of this window is on one of the `displays` to grab it, so a monitor that is
    /// no longer there never leaves a window out of reach.
    pub fn is_reachable(&self, displays: &[WindowBounds]) -> bool {
        self.width >= 200.0
            && self.height >= 120.0
            && displays.iter().any(|d| {
                let w = (self.x + self.width).min(d.x + d.width) - self.x.max(d.x);
                let h = (self.y + self.height).min(d.y + d.height) - self.y.max(d.y);
                w >= 120.0 && h >= 60.0
            })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StoreData {
    pub settings: Settings,
    pub workspaces: Option<PersistedWorkspaces>,
    pub widget_bounds: HashMap<String, WindowBounds>,
    /// Last known folder of each terminal's shell.
    pub pane_cwds: HashMap<String, String>,
    pub window_bounds: HashMap<String, WindowBounds>,
}

// ---- Helpers ------------------------------------------------------------------------------

/// A short unique id for a workspace or a terminal.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}{:x}{:x}", fastrand::u32(..), fastrand::u16(..))
}

impl Workspace {
    pub fn new(name: impl Into<String>, window_id: &str) -> Self {
        Self {
            id: new_id("w"),
            name: name.into(),
            window_id: window_id.to_string(),
            parked: false,
            folder: String::new(),
            layout: LayoutId::Auto,
            split: None,
            panes: Vec::new(),
            active_pane_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real file written by the Electron app.
    const ELECTRON_FILE: &str = r#"{
      "settings": {
        "version": 2,
        "appearance": {"theme": "system"},
        "window": {"startMaximized": true},
        "terminal": {"font": "geist-mono", "customFontFamily": "", "fontSize": 13, "lineHeight": 1.25,
          "cursorStyle": "block", "cursorBlink": false, "scrollback": 10000, "shell": "",
          "startDirectory": "", "optionAsMeta": false, "copyOnSelect": false, "suggestions": true},
        "workspaces": {"restoreOnLaunch": true, "autoColor": true},
        "updates": {"autoCheck": true},
        "resources": {"intervalMs": 2000, "networkUnit": "bytes", "popoutAlwaysOnTop": true,
          "showInSidebar": {"cpu": true, "ram": true, "disk": true, "net": true}}
      },
      "workspaces": {
        "workspaces": [
          {"id": "w-a", "name": "Selexi", "windowId": "main", "parked": false, "folder": "/tmp",
           "layout": "main-left", "split": 0.55,
           "panes": [
             {"id": "p1", "name": "API", "note": "invoice export", "color": "cyan"},
             {"id": "p2", "name": "", "note": "", "color": null}
           ],
           "activePaneId": "p1"},
          {"id": "w-b", "name": "Parked one", "windowId": "w2", "parked": true, "folder": "",
           "layout": "something-new", "split": null, "panes": [], "activePaneId": null}
        ],
        "activeByWindow": {"main": "w-a"}
      },
      "widgetBounds": {"cpu": {"x": 10, "y": 20, "width": 320, "height": 190}},
      "paneCwds": {"p1": "/Users/me/project"},
      "windowBounds": {"main": {"x": 0, "y": 25, "width": 1512, "height": 900}}
    }"#;

    #[test]
    fn reads_a_file_written_by_the_electron_app() {
        let data: StoreData = serde_json::from_str(ELECTRON_FILE).unwrap();
        let ws = data.workspaces.as_ref().unwrap();
        assert_eq!(ws.workspaces.len(), 2);
        assert_eq!(ws.workspaces[0].layout, LayoutId::MainLeft);
        assert_eq!(ws.workspaces[0].split, Some(0.55));
        assert_eq!(ws.workspaces[0].panes[0].color, Some(PaneColor::Cyan));
        assert_eq!(ws.workspaces[0].panes[1].color, None);
        // An unknown layout from a newer version becomes Auto instead of failing.
        assert_eq!(ws.workspaces[1].layout, LayoutId::Auto);
        assert_eq!(ws.workspaces[1].window_id, "w2");
        assert_eq!(ws.active_by_window["main"], "w-a");
        assert_eq!(data.pane_cwds["p1"], "/Users/me/project");
        assert_eq!(data.window_bounds["main"].width, 1512.0);
        assert!(data.settings.terminal.suggestions);
    }

    #[test]
    fn writes_what_it_reads() {
        let data: StoreData = serde_json::from_str(ELECTRON_FILE).unwrap();
        let again: StoreData = serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        assert_eq!(data, again);
    }

    #[test]
    fn an_unknown_color_or_missing_parts_do_not_break_loading() {
        let json = r#"{"workspaces": {"workspaces": [
            {"id": "w", "name": "x", "panes": [{"id": "p", "color": "magenta"}]}]}}"#;
        let data: StoreData = serde_json::from_str(json).unwrap();
        let ws = &data.workspaces.unwrap().workspaces[0];
        assert_eq!(ws.panes[0].color, None);
        assert_eq!(ws.window_id, "main");
        assert_eq!(data.settings, Settings::default());
    }

    #[test]
    fn settings_from_an_old_version_are_brought_back_in_range() {
        let mut s = Settings::default();
        s.version = 1;
        s.terminal.option_as_meta = true;
        s.terminal.font_size = 200.0;
        s.terminal.scrollback = 1;
        s.resources.interval_ms = 7;
        let s = s.normalized();
        assert_eq!(s.version, SETTINGS_VERSION);
        assert!(!s.terminal.option_as_meta);
        assert_eq!(s.terminal.font_size, 32.0);
        assert_eq!(s.terminal.scrollback, 500);
        assert_eq!(s.resources.interval_ms, 2000);
    }

    #[test]
    fn ids_are_unique() {
        let ids: std::collections::HashSet<_> = (0..1000).map(|_| new_id("p")).collect();
        assert_eq!(ids.len(), 1000);
    }

    #[test]
    fn a_window_on_a_missing_monitor_is_not_reachable() {
        let screen = WindowBounds { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0, maximized: false };
        let on_screen = WindowBounds { x: 100.0, y: 80.0, width: 1200.0, height: 800.0, maximized: false };
        let far_right = WindowBounds { x: 3000.0, y: 100.0, width: 1200.0, height: 800.0, maximized: false };
        let barely = WindowBounds { x: 1900.0, y: 100.0, width: 1200.0, height: 800.0, maximized: false };
        let tiny = WindowBounds { x: 10.0, y: 10.0, width: 50.0, height: 50.0, maximized: false };
        assert!(on_screen.is_reachable(&[screen]));
        assert!(!far_right.is_reachable(&[screen]));
        assert!(!barely.is_reachable(&[screen]));
        assert!(!tiny.is_reachable(&[screen]));
        assert!(far_right.is_reachable(&[screen, WindowBounds { x: 1920.0, width: 2560.0, ..screen }]));
    }

    #[test]
    fn old_bounds_without_the_maximized_flag_still_load() {
        let b: WindowBounds = serde_json::from_str(r#"{"x":10,"y":20,"width":800,"height":600}"#).unwrap();
        assert!(!b.maximized);
        assert_eq!(b.width, 800.0);
    }
}
