//! sdterm native: first spike. Four real terminals in a 2x2 grid, each with a name and a color,
//! to find out how GPUI + alacritty_terminal + portable-pty feel on this machine before building
//! the rest of the app.

mod layout;
mod model;
mod store;

use std::sync::Arc;

use anyhow::Result;
use gpui::{
    App, AppContext, Context, Edges, Entity, FontWeight, Hsla, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, Styled, Subscription, Window, div, px, rgb,
    transparent_black,
};
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

/// Variables that describe whatever launched sdterm, not the user's terminal. Same list as the
/// Electron app: they must not leak into the shells.
fn is_launcher_var(key: &str) -> bool {
    matches!(key, "CLAUDECODE" | "CLAUDE_PID" | "CLAUDE_EFFORT" | "INIT_CWD")
        || key.starts_with("npm_")
        || key.starts_with("ELECTRON_")
        || matches!(
            key,
            "CLAUDE_CODE_CHILD_SESSION" | "CLAUDE_CODE_ENTRYPOINT" | "CLAUDE_CODE_EXECPATH"
        )
        || key.starts_with("CLAUDE_CODE_SESSION_")
        || key.starts_with("CLAUDE_CODE_MESSAGING_")
}

/// Starts a login shell in a new PTY and returns a terminal view wired to it.
fn spawn_terminal(
    cx: &mut App,
    window: &mut Window,
    config: TerminalConfig,
) -> Result<Entity<TerminalView>> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let pair = native_pty_system().openpty(PtySize {
        rows: config.rows as u16,
        cols: config.cols as u16,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l"); // login shell, like Terminal.app: PATH comes from the user's profile
    for (key, _) in std::env::vars() {
        if is_launcher_var(&key) {
            cmd.env_remove(key);
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "sdterm");
    if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if let Some(home) = std::env::var_os("HOME") {
        cmd.cwd(home);
    }

    let _child = pair.slave.spawn_command(cmd)?;
    let writer = pair.master.take_writer()?;
    let reader = pair.master.try_clone_reader()?;
    let master = Arc::new(parking_lot::Mutex::new(pair.master));
    drop(pair.slave);

    let for_resize = master.clone();
    let terminal = cx.new(|cx| {
        TerminalView::new(writer, reader, config, cx)
            .with_resize_callback(move |cols, rows| {
                let _ = for_resize.lock().resize(PtySize {
                    rows: rows as u16,
                    cols: cols as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            })
            .with_exit_callback(|_window, _cx| {})
    });
    let _ = window;
    Ok(terminal)
}

fn palette() -> ColorPalette {
    // The dark theme of the Electron app: near black background, cyan cursor.
    ColorPalette::builder()
        .background(0x14, 0x14, 0x14)
        .foreground(0xE6, 0xE6, 0xE6)
        .cursor(0x52, 0xE5, 0xF1)
        .black(0x1D, 0x1D, 0x1D)
        .red(0xEF, 0x7A, 0x7A)
        .green(0x6F, 0xD0, 0x86)
        .yellow(0xE6, 0xC0, 0x6A)
        .blue(0x6E, 0x9B, 0xF0)
        .magenta(0xD0, 0x8A, 0xE6)
        .cyan(0x52, 0xE5, 0xF1)
        .white(0xC4, 0xC4, 0xC4)
        .bright_black(0x5A, 0x5A, 0x5A)
        .bright_red(0xFF, 0x9A, 0x9A)
        .bright_green(0x8C, 0xE8, 0xA0)
        .bright_yellow(0xF5, 0xD8, 0x8A)
        .bright_blue(0x8F, 0xB4, 0xFF)
        .bright_magenta(0xE6, 0xA8, 0xF5)
        .bright_cyan(0x8A, 0xF0, 0xF8)
        .bright_white(0xF0, 0xF0, 0xF0)
        .build()
}

struct Pane {
    name: &'static str,
    color: u32,
    terminal: Entity<TerminalView>,
}

struct Grid {
    panes: Vec<Pane>,
    /// Keeps the focus listeners alive: the grid redraws when the focused terminal changes.
    _subs: Vec<Subscription>,
}

/// Mixes two 0xRRGGBB colors: `t` of `b` over `a`.
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let (x, y) = (((a >> shift) & 0xFF) as f32, ((b >> shift) & 0xFF) as f32);
        ((x + (y - x) * t).round() as u32) & 0xFF
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

impl Render for Grid {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Same cues as the Electron app: the color bar is solid and the title bright on the
        // terminal you are typing in or dictating to, and dimmed on the others.
        let mut cells: Vec<_> = (0..self.panes.len())
            .map(|i| {
                let pane = &self.panes[i];
                let focused = pane.terminal.read(cx).focus_handle().is_focused(window);
                let accent: Hsla = rgb(pane.color).into();
                div()
                    .id(("pane", i))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(if focused {
                        accent.opacity(0.55)
                    } else {
                        transparent_black()
                    })
                    // A click anywhere on the pane, title included, moves the focus there.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            let handle = this.panes[i].terminal.read(cx).focus_handle().clone();
                            window.focus(&handle);
                        }),
                    )
                    .child(div().h(px(2.)).bg(accent.opacity(if focused { 1.0 } else { 0.4 })))
                    .child(
                        div()
                            .h(px(26.))
                            .px_3()
                            .flex()
                            .items_center()
                            .bg(rgb(if focused {
                                mix(0x1B1B1B, pane.color, 0.14)
                            } else {
                                0x1B1B1B
                            }))
                            .text_color(rgb(if focused { 0xF2F2F2 } else { 0x8A8A8A }))
                            .font_weight(if focused {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_size(px(12.))
                            .child(pane.name),
                    )
                    .child(div().flex_1().child(pane.terminal.clone()))
            })
            .collect();

        let row = |a, b| div().flex_1().flex().flex_row().gap_px().child(a).child(b);
        let (c, d) = (cells.remove(2), cells.remove(2));
        let (a, b) = (cells.remove(0), cells.remove(0));
        div()
            .size_full()
            .bg(rgb(0x2A2A2A))
            .flex()
            .flex_col()
            .gap_px()
            .child(row(a, b))
            .child(row(c, d))
    }
}

fn main() {
    gpui::Application::new().run(|cx: &mut App| {
        cx.spawn(async move |cx| {
            cx.open_window(
                gpui::WindowOptions {
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("sdterm native".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let config = || TerminalConfig {
                        font_family: "Menlo".into(),
                        font_size: px(13.0),
                        cols: 80,
                        rows: 24,
                        scrollback: 10_000,
                        line_height_multiplier: 1.25,
                        padding: Edges::all(px(8.0)),
                        colors: palette(),
                    };
                    let specs = [
                        ("API", 0x52E5F1),
                        ("Web", 0x6FD086),
                        ("Build", 0xE6C06A),
                        ("Shell", 0x8F8F8F),
                    ];
                    let panes = specs
                        .into_iter()
                        .map(|(name, color)| Pane {
                            name,
                            color,
                            terminal: spawn_terminal(cx, window, config())
                                .expect("could not start a shell"),
                        })
                        .collect::<Vec<_>>();
                    panes[0].terminal.read(cx).focus_handle().focus(window);
                    cx.new(|cx| {
                        let mut subs = Vec::new();
                        for pane in &panes {
                            let handle = pane.terminal.read(cx).focus_handle().clone();
                            subs.push(cx.on_focus_in(&handle, window, |_, _, cx| cx.notify()));
                            subs.push(cx.on_blur(&handle, window, |_, _, cx| cx.notify()));
                        }
                        Grid { panes, _subs: subs }
                    })
                },
            )?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
