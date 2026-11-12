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

use crate::model::{CursorStyle, FontChoice, TerminalSettings};
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

/// What PowerShell runs at startup, after the user's own profile: the prompt also tells sdterm the
/// folder (OSC 9;9, the same sequence Windows Terminal uses), so a terminal comes back where it was.
/// The original prompt runs first so a theme that looks at the result of the last command still
/// sees it. With suggestions on, history predictions are turned on where PSReadLine has them.
const POWERSHELL_INTEGRATION: &str = r#"
$global:__sdterm_prompt = $function:prompt
function global:prompt {
    $shown = & $global:__sdterm_prompt
    $where = $executionContext.SessionState.Path.CurrentLocation
    if ($where.Provider.Name -eq 'FileSystem') {
        $shown = "$([char]27)]9;9;`"$($where.ProviderPath)`"$([char]27)\$shown"
    }
    $shown
}
"#;

const POWERSHELL_SUGGESTIONS: &str = r#"
try { Set-PSReadLineOption -PredictionSource History } catch { }
"#;

/// The program name of a shell path without its folder and `.exe`, lower case. Both kinds of
/// slash count, so a Windows path is read the same way everywhere.
fn shell_name(shell: &str) -> String {
    let file = shell.rsplit(['/', '\\']).next().unwrap_or("").to_ascii_lowercase();
    file.strip_suffix(".exe").unwrap_or(&file).to_string()
}

/// The arguments that start PowerShell with the integration. It goes in as an encoded command
/// (UTF-16 in base64), so no quoting can break it, and the profile still loads.
fn powershell_args(shell: &str, suggestions: bool) -> Option<Vec<String>> {
    use base64::Engine;
    if !matches!(shell_name(shell).as_str(), "powershell" | "pwsh") {
        return None;
    }
    let mut script = POWERSHELL_INTEGRATION.to_string();
    if suggestions {
        script.push_str(POWERSHELL_SUGGESTIONS);
    }
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Some(vec![
        "-NoLogo".into(),
        "-NoExit".into(),
        "-EncodedCommand".into(),
        base64::engine::general_purpose::STANDARD.encode(utf16),
    ])
}

/// cmd.exe reports its folder through the prompt string (OSC 9;9 wrapped around the usual `$P$G`).
/// A prompt the user set themselves is left alone.
const CMD_PROMPT: &str = "$E]9;9;$P$E\\$P$G";

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
        FontChoice::GeistMono => "Geist Mono".to_string(),
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
        cursor_shape: match settings.cursor_style {
            CursorStyle::Block => gpui_terminal::CursorShape::Block,
            CursorStyle::Bar => gpui_terminal::CursorShape::Beam,
            CursorStyle::Underline => gpui_terminal::CursorShape::Underline,
        },
        cursor_blink: settings.cursor_blink,
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
    if let Some(args) = powershell_args(&shell, settings.suggestions) {
        cmd.args(args);
    } else if !cfg!(windows) {
        cmd.arg("-l"); // a login shell, like Terminal.app: PATH comes from the user's profile
    }
    if shell_name(&shell) == "cmd" && std::env::var_os("PROMPT").is_none() {
        cmd.env("PROMPT", CMD_PROMPT);
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn powershell_gets_the_integration_as_an_encoded_command() {
        let args = powershell_args(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe", true).unwrap();
        assert_eq!(&args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
        let bytes = base64::engine::general_purpose::STANDARD.decode(&args[3]).unwrap();
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let script = String::from_utf16(&units).unwrap();
        assert!(script.contains("function global:prompt"));
        assert!(script.contains("]9;9;"));
        assert!(script.contains("PredictionSource History"));
        let without = powershell_args("pwsh", false).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(&without[3]).unwrap();
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert!(!String::from_utf16(&units).unwrap().contains("PredictionSource"));
    }

    #[test]
    fn other_shells_are_left_alone() {
        assert!(powershell_args("/bin/zsh", true).is_none());
        assert!(powershell_args("cmd.exe", true).is_none());
        assert_eq!(shell_name("C:\\Windows\\System32\\cmd.exe"), "cmd");
    }
}
