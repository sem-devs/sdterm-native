//! A running shell and the terminal view that shows it.
//!
//! Shells belong to the app, not to a window or to what is on screen: switching workspace hides a
//! terminal without touching its shell, and only closing it, parking its workspace or quitting
//! ends the process.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Result;
use gpui::{App, AppContext, Edges, Entity, px};
use gpui_terminal::{TerminalConfig, TerminalView};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

use crate::model::{FontChoice, TerminalSettings};
use crate::store;
use crate::theme::Theme;

pub struct Session {
    pub view: Entity<TerminalView>,
    child: Box<dyn Child + Send + Sync>,
}

impl Session {
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Ends the shell. Dropping the view afterwards closes the PTY.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

/// Variables that describe whatever launched sdterm, not the user's terminal. They must not leak
/// into shells: a Claude Code started inside would think it is a child of another session, and
/// npm and Electron variables confuse project tooling.
fn is_launcher_var(key: &str) -> bool {
    matches!(
        key,
        "CLAUDECODE"
            | "CLAUDE_PID"
            | "CLAUDE_EFFORT"
            | "INIT_CWD"
            | "CLAUDE_CODE_CHILD_SESSION"
            | "CLAUDE_CODE_ENTRYPOINT"
            | "CLAUDE_CODE_EXECPATH"
    ) || key.starts_with("npm_")
        || key.starts_with("ELECTRON_")
        || key.starts_with("CLAUDE_CODE_SESSION_")
        || key.starts_with("CLAUDE_CODE_MESSAGING_")
}

pub fn default_shell() -> String {
    if cfg!(windows) {
        return "powershell.exe".into();
    }
    std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") { "/bin/zsh".into() } else { "/bin/bash".into() }
    })
}

/// The shell chosen in Settings. A typo there must not leave the user without a terminal.
fn chosen_shell(setting: &str) -> String {
    let wanted = setting.trim();
    if wanted.is_empty() {
        return default_shell();
    }
    let has_path = wanted.contains('/') || wanted.contains('\\');
    if has_path && !Path::new(wanted).exists() {
        return default_shell();
    }
    wanted.to_string()
}

/// Where the files we hand to shells live: inside the app bundle, or in the repo while developing.
fn resources_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bundle) = exe.parent().and_then(|p| p.parent()).map(|p| p.join("Resources")) {
            if bundle.join("shell").exists() {
                return bundle;
            }
        }
        if let Some(beside) = exe.parent().map(|p| p.join("resources")) {
            if beside.join("shell").exists() {
                return beside;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
}

/// zsh gets a startup folder that runs the user's own files first and then adds command
/// suggestions and Tab completion. Their ~/.zshrc is never edited.
fn suggestion_env(cmd: &mut CommandBuilder, shell: &str) {
    let name = Path::new(shell).file_name().and_then(|n| n.to_str()).unwrap_or("");
    if cfg!(windows) || name != "zsh" {
        return;
    }
    let shell_dir = resources_dir().join("shell");
    if !shell_dir.join("zsh").join(".zshrc").exists() {
        return;
    }
    let state = store::data_dir().join("shell");
    if std::fs::create_dir_all(&state).is_err() {
        return;
    }
    cmd.env("ZDOTDIR", shell_dir.join("zsh"));
    cmd.env("SDTERM_ZDOTDIR_ORIG", std::env::var("ZDOTDIR").unwrap_or_default());
    cmd.env("SDTERM_SHELL_DIR", &shell_dir);
    cmd.env("SDTERM_STATE_DIR", &state);
    cmd.env("SDTERM_SUGGESTIONS", "1");
}

fn mono_font(settings: &TerminalSettings) -> String {
    let platform = if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(windows) {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    };
    match settings.font {
        FontChoice::Custom if !settings.custom_font_family.trim().is_empty() => {
            settings.custom_font_family.trim().to_string()
        }
        // The bundled Geist Mono comes with the polish step; until then the system mono font.
        _ => platform.to_string(),
    }
}

pub fn terminal_config(settings: &TerminalSettings, theme: &Theme) -> TerminalConfig {
    TerminalConfig {
        font_family: mono_font(settings),
        font_size: px(settings.font_size as f32),
        cols: 80,
        rows: 24,
        scrollback: settings.scrollback as usize,
        line_height_multiplier: settings.line_height as f32,
        padding: Edges::all(px(8.0)),
        colors: theme.terminal_palette(),
    }
}

/// Starts a shell in `cwd` and returns it with its terminal view. `on_exit` runs when the shell
/// ends by itself (the user typed `exit`).
pub fn spawn(
    cx: &mut App,
    settings: &TerminalSettings,
    theme: &Theme,
    cwd: &Path,
    on_exit: impl Fn(&mut App) + 'static,
) -> Result<Session> {
    let config = terminal_config(settings, theme);
    let pair = native_pty_system().openpty(PtySize {
        rows: config.rows as u16,
        cols: config.cols as u16,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let shell = chosen_shell(&settings.shell);
    let mut cmd = CommandBuilder::new(&shell);
    if !cfg!(windows) {
        cmd.arg("-l"); // a login shell, like Terminal.app: PATH comes from the user's profile
    }
    for (key, _) in std::env::vars() {
        if is_launcher_var(&key) {
            cmd.env_remove(key);
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "sdterm");
    cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if settings.suggestions {
        suggestion_env(&mut cmd, &shell);
    }
    cmd.cwd(cwd);

    let child = pair.slave.spawn_command(cmd)?;
    let writer = pair.master.take_writer()?;
    let reader = pair.master.try_clone_reader()?;
    let master = Arc::new(parking_lot::Mutex::new(pair.master));
    drop(pair.slave);

    let on_exit: Rc<dyn Fn(&mut App)> = Rc::new(on_exit);
    let option_as_meta = settings.option_as_meta;
    let copy_on_select = settings.copy_on_select;
    let view = cx.new(|cx| {
        TerminalView::new(writer, reader, config, cx)
            .with_option_as_meta(option_as_meta)
            .with_copy_on_select(copy_on_select)
            .with_resize_callback(move |cols, rows| {
                let _ = master.lock().resize(PtySize {
                    rows: rows as u16,
                    cols: cols as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            })
            .with_exit_callback(move |_window, cx| {
                // Later, outside this view's own update, so closing the pane can drop the view.
                let on_exit = on_exit.clone();
                cx.defer(move |cx| on_exit(cx));
            })
    });
    Ok(Session { view, child })
}
