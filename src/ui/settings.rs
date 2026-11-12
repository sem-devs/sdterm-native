//! The settings page. Every change applies at once to the terminals that are open.

use gpui::prelude::*;
use gpui::{AnyElement, App, Entity, FontWeight, PathPromptOptions, div, px};

use super::controls::{choice, row, section, stepper, toggle};
use super::parts::text_button;
use super::{MAC, Snapshot, VERSION};
use crate::model::{CursorStyle, FontChoice, NetworkUnit, Settings, ThemeMode};
use crate::state::{AppState, SETTINGS_SECTIONS, SettingsSection};
use crate::theme::Theme;

const WEBSITE: &str = "https://sem-devs.com";
const EMAIL: &str = "mailto:info@sem-devs.com";
const GITHUB: &str = "https://github.com/sem-devs";
const REVIEW: &str = "https://g.page/r/CUJ_bpXLg9UKEBI/review";

/// A change to the settings, from a click.
fn set(state: &Entity<AppState>, change: impl Fn(&mut Settings) + 'static) -> impl Fn(&mut App) + 'static {
    let state = state.clone();
    move |cx| state.update(cx, |s, cx| s.update_settings(cx, &change))
}

fn on_off<F>(
    id: &'static str,
    on: bool,
    theme: &Theme,
    state: &Entity<AppState>,
    change: F,
) -> impl IntoElement
where
    F: Fn(&mut Settings, bool) + Clone + 'static,
{
    let state = state.clone();
    toggle(id, on, theme, move |value, cx| {
        let change = change.clone();
        state.update(cx, |s, cx| s.update_settings(cx, move |settings| change(settings, value)))
    })
}

pub fn settings_view(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let mut nav = div().flex().flex_col().gap_0p5().w(px(180.)).flex_none().p_3();
    for (i, (section, label)) in SETTINGS_SECTIONS.iter().enumerate() {
        let selected = snap.settings_section == *section;
        let (state, section) = (state.clone(), *section);
        let (hover, fg, muted) = (theme.accent.opacity(0.6), theme.foreground, theme.muted_foreground);
        nav = nav.child(
            div()
                .id(("settings-nav", i))
                .flex()
                .items_center()
                .h(px(30.))
                .px_3()
                .rounded_md()
                .cursor_pointer()
                .text_color(if selected { fg } else { muted })
                .when(selected, |d| d.bg(theme.accent))
                .when(!selected, |d| d.hover(move |s| s.bg(hover).text_color(fg)))
                .child(*label)
                .on_click(move |_, _, cx| state.update(cx, |s, cx| s.set_settings_section(section, cx))),
        );
    }

    let content = match snap.settings_section {
        SettingsSection::Appearance => appearance(snap, theme, state),
        SettingsSection::Terminal => terminal(snap, theme, state),
        SettingsSection::Workspaces => workspaces(snap, theme, state),
        SettingsSection::Resources => resources(snap, theme, state),
        SettingsSection::Updates => updates(snap, theme, state),
        SettingsSection::Shortcuts => shortcuts(theme),
        SettingsSection::About => about(snap, theme, state),
    };

    div()
        .size_full()
        .flex()
        .child(nav)
        .child(
            div()
                .id("settings-content")
                .flex_1()
                .min_w(px(0.))
                .overflow_y_scroll()
                .px_8()
                .py_6()
                .child(div().max_w(px(720.)).child(content)),
        )
        .into_any_element()
}

fn appearance(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let s = &snap.settings;
    div()
        .child(section(
            theme,
            "Appearance",
            Some("sdterm follows the system by default."),
            vec![
                row(
                    theme,
                    "Theme",
                    None,
                    choice(
                        "theme",
                        &[(ThemeMode::System, "System"), (ThemeMode::Light, "Light"), (ThemeMode::Dark, "Dark")],
                        s.appearance.theme,
                        theme,
                        {
                            let state = state.clone();
                            move |mode, cx| {
                                state.update(cx, |st, cx| st.update_settings(cx, move |s| s.appearance.theme = mode))
                            }
                        },
                    ),
                ),
                row(
                    theme,
                    "Open as large as the screen",
                    Some("A normal resizable window, not full-screen mode. Applies the next time sdterm starts."),
                    on_off("maximized", s.window.start_maximized, theme, state, |s, v| s.window.start_maximized = v),
                ),
            ],
        ))
        .into_any_element()
}

fn text_field(snap: &Snapshot, key: &'static str) -> AnyElement {
    match snap.setting_inputs.get(key) {
        Some(input) => div().w(px(300.)).child(input.clone()).into_any_element(),
        None => div().into_any_element(),
    }
}

/// Asks for a folder and calls `done` with it.
fn pick_folder(cx: &mut App, done: impl FnOnce(String, &mut gpui::AsyncApp) + 'static) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Choose the folder new terminals start in".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = picked.await {
            if let Some(path) = paths.first() {
                done(path.to_string_lossy().to_string(), cx);
            }
        }
    })
    .detach();
}

fn terminal(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let t = &snap.settings.terminal;
    let mut text_rows = vec![
        row(
            theme,
            "Font",
            Some("Applied to every open terminal right away."),
            choice(
                "font",
                &[(FontChoice::GeistMono, "Geist Mono"), (FontChoice::System, "System"), (FontChoice::Custom, "Custom")],
                t.font,
                theme,
                {
                    let state = state.clone();
                    move |font, cx| state.update(cx, |st, cx| st.update_settings(cx, move |s| s.terminal.font = font))
                },
            ),
        ),
    ];
    if t.font == FontChoice::Custom {
        text_rows.push(row(
            theme,
            "Font family",
            Some("The name of a monospaced font installed on this computer."),
            text_field(snap, "custom_font"),
        ));
    }
    text_rows.push(row(
        theme,
        "Font size",
        None,
        stepper(
            "size",
            format!("{} px", t.font_size),
            theme,
            set(state, |s| s.terminal.font_size -= 1.0),
            set(state, |s| s.terminal.font_size += 1.0),
        ),
    ));
    text_rows.push(row(
        theme,
        "Line height",
        None,
        stepper(
            "height",
            format!("{:.2}", t.line_height),
            theme,
            set(state, |s| s.terminal.line_height -= 0.05),
            set(state, |s| s.terminal.line_height += 0.05),
        ),
    ));
    text_rows.push(row(
        theme,
        "Cursor",
        None,
        choice(
            "cursor",
            &[(CursorStyle::Block, "Block"), (CursorStyle::Bar, "Bar"), (CursorStyle::Underline, "Underline")],
            t.cursor_style,
            theme,
            {
                let state = state.clone();
                move |style, cx| state.update(cx, |st, cx| st.update_settings(cx, move |s| s.terminal.cursor_style = style))
            },
        ),
    ));
    text_rows.push(row(
        theme,
        "Blinking cursor",
        Some("The cursor blinks while the terminal has the keyboard focus. Programs can still ask for it."),
        on_off("blink", t.cursor_blink, theme, state, |s, v| s.terminal.cursor_blink = v),
    ));
    text_rows.push(row(
        theme,
        "Scrollback",
        Some("Lines kept per terminal, for scrolling back. Applies to terminals you open next."),
        choice(
            "scrollback",
            &[(1_000u32, "1k"), (5_000, "5k"), (10_000, "10k"), (50_000, "50k"), (200_000, "200k")],
            t.scrollback,
            theme,
            {
                let state = state.clone();
                move |n, cx| state.update(cx, |st, cx| st.update_settings(cx, move |s| s.terminal.scrollback = n))
            },
        ),
    ));

    let choose = {
        let state = state.clone();
        text_button("choose-dir", "Choose…", theme, false, move |cx| {
            let state = state.clone();
            pick_folder(cx, move |folder, cx| {
                state.update(cx, |s, cx| s.update_settings(cx, move |s| s.terminal.start_directory = folder)).ok();
            });
        })
    };

    let mut behavior = Vec::new();
    if MAC {
        behavior.push(row(
            theme,
            "Option key as Meta",
            Some("Off by default, like Terminal.app, so Option types characters such as @ # [ ] on Italian and other layouts. Turn it on if you use Option as Alt in vim or emacs."),
            on_off("meta", t.option_as_meta, theme, state, |s, v| s.terminal.option_as_meta = v),
        ));
    }
    behavior.push(row(
        theme,
        "Copy on select",
        Some("Copies text to the clipboard as soon as you select it."),
        on_off("copy-select", t.copy_on_select, theme, state, |s, v| s.terminal.copy_on_select = v),
    ));
    if !cfg!(windows) {
        behavior.push(row(
            theme,
            "Command suggestions",
            Some("Shows commands you ran before in grey as you type (press the right arrow to accept) and completes git branches, npm scripts and more with Tab. Works with zsh, the default on macOS, and applies to terminals you open next. Your own shell files are left alone."),
            on_off("suggestions", t.suggestions, theme, state, |s, v| s.terminal.suggestions = v),
        ));
    }

    div()
        .child(section(theme, "Text", None, text_rows))
        .child(section(
            theme,
            "Shell",
            Some("Applies to terminals you open next."),
            vec![
                row(
                    theme,
                    "Shell",
                    Some("Empty uses your login shell (PowerShell on Windows). A name like fish, or a full path."),
                    text_field(snap, "shell"),
                ),
                row(
                    theme,
                    "Start folder",
                    Some("Where a terminal without a project folder starts. Empty is your home folder."),
                    div().flex().items_center().gap_2().child(text_field(snap, "start_directory")).child(choose),
                ),
            ],
        ))
        .child(section(theme, "Behavior", None, behavior))
        .into_any_element()
}

fn workspaces(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let w = &snap.settings.workspaces;
    div()
        .child(section(
            theme,
            "Workspaces",
            None,
            vec![
                row(
                    theme,
                    "Reopen workspaces when sdterm starts",
                    Some("Every workspace, terminal, name and note is there again, and each terminal starts in the folder it was in."),
                    on_off("restore", w.restore_on_launch, theme, state, |s, v| s.workspaces.restore_on_launch = v),
                ),
                row(
                    theme,
                    "Give new terminals a color",
                    Some("Each new terminal takes the next free color, so you can tell them apart."),
                    on_off("auto-color", w.auto_color, theme, state, |s, v| s.workspaces.auto_color = v),
                ),
            ],
        ))
        .into_any_element()
}

fn resources(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let r = &snap.settings.resources;
    let m = &r.show_in_sidebar;
    div()
        .child(section(
            theme,
            "Resources",
            Some("CPU, RAM, disk and network, in the sidebar and as pop-out windows."),
            vec![
                row(
                    theme,
                    "Update every",
                    None,
                    choice(
                        "interval",
                        &[(1000u32, "1 s"), (2000, "2 s"), (5000, "5 s")],
                        r.interval_ms,
                        theme,
                        {
                            let state = state.clone();
                            move |ms, cx| state.update(cx, |st, cx| st.update_settings(cx, move |s| s.resources.interval_ms = ms))
                        },
                    ),
                ),
                row(
                    theme,
                    "Network speed in",
                    None,
                    choice(
                        "unit",
                        &[(NetworkUnit::Bytes, "Bytes/s"), (NetworkUnit::Bits, "Bits/s")],
                        r.network_unit,
                        theme,
                        {
                            let state = state.clone();
                            move |unit, cx| state.update(cx, |st, cx| st.update_settings(cx, move |s| s.resources.network_unit = unit))
                        },
                    ),
                ),
                row(
                    theme,
                    "Keep pop-outs on top",
                    Some("Pop-out monitor windows stay above other windows."),
                    on_off("on-top", r.popout_always_on_top, theme, state, |s, v| s.resources.popout_always_on_top = v),
                ),
            ],
        ))
        .child(section(
            theme,
            "Show in the sidebar",
            None,
            vec![
                row(theme, "CPU", None, on_off("sb-cpu", m.cpu, theme, state, |s, v| s.resources.show_in_sidebar.cpu = v)),
                row(theme, "RAM", None, on_off("sb-ram", m.ram, theme, state, |s, v| s.resources.show_in_sidebar.ram = v)),
                row(theme, "Disk", None, on_off("sb-disk", m.disk, theme, state, |s, v| s.resources.show_in_sidebar.disk = v)),
                row(theme, "Network", None, on_off("sb-net", m.net, theme, state, |s, v| s.resources.show_in_sidebar.net = v)),
            ],
        ))
        .into_any_element()
}

fn updates(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    use crate::updater::UpdateStatus;
    let s = &snap.settings;
    let status = match &snap.update {
        UpdateStatus::Idle => "Not checked yet.".to_string(),
        UpdateStatus::Checking => "Checking…".to_string(),
        UpdateStatus::UpToDate => "You are up to date.".to_string(),
        UpdateStatus::Available { version, .. } => format!("Version {version} is available."),
        UpdateStatus::Downloading(n) => format!("Downloading the update: {n}%"),
        UpdateStatus::Error(message) => format!("The check did not work: {message}"),
    };
    let check = state.clone();
    let mut actions = div().flex().items_center().gap_2().child(text_button("check-now", "Check now", theme, false, move |cx| {
        check.update(cx, |s, cx| s.check_for_update(true, cx))
    }));
    if matches!(snap.update, UpdateStatus::Available { .. }) {
        let install = state.clone();
        actions = actions.child(text_button(
            "install-now",
            if cfg!(target_os = "macos") { "Update and restart" } else { "Download" },
            theme,
            true,
            move |cx| install.update(cx, |s, cx| s.install_update(cx)),
        ));
    }
    div()
        .child(section(
            theme,
            "Updates",
            Some("sdterm tells you when a new version is out and updates itself when you say so."),
            vec![
                row(
                    theme,
                    "Check for updates automatically",
                    None,
                    on_off("auto-check", s.updates.auto_check, theme, state, |s, v| s.updates.auto_check = v),
                ),
                row(theme, "Version", Some("This is the version you are running."), div().font_weight(FontWeight::MEDIUM).child(VERSION.to_string())),
                row(theme, "Status", Some(status_hint()), div().flex().flex_col().items_end().gap_2().child(div().text_color(theme.muted_foreground).child(status)).child(actions)),
            ],
        ))
        .into_any_element()
}

fn status_hint() -> &'static str {
    if cfg!(debug_assertions) {
        "This is a development build, so it does not look for updates on its own."
    } else {
        "sdterm looks at startup and every few hours."
    }
}

fn shortcuts(theme: &Theme) -> AnyElement {
    let m = if MAC { "⌘" } else { "Ctrl+Shift+" };
    let rows: Vec<(&'static str, String)> = vec![
        ("New terminal", format!("{m}T")),
        ("Close terminal", format!("{m}W")),
        ("New workspace", format!("{m}N")),
        ("Show or hide the sidebar", format!("{m}B")),
        ("Settings", format!("{m},")),
        ("Copy", if MAC { "⌘C".into() } else { "Ctrl+Shift+C".into() }),
        ("Paste", if MAC { "⌘V".into() } else { "Ctrl+Shift+V".into() }),
        ("Find in the terminal", if MAC { "⌘F".into() } else { "Ctrl+Shift+F".into() }),
        ("Next or previous match", "Enter, Shift+Enter".into()),
        ("Open a link", if MAC { "⌘ + click".into() } else { "Ctrl + click".into() }),
    ];
    let mut list = Vec::new();
    for (label, keys) in rows {
        list.push(row(
            theme,
            label,
            None,
            div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .border_1()
                .border_color(theme.input)
                .bg(theme.muted)
                .text_size(px(12.))
                .child(keys),
        ));
    }
    div()
        .child(section(theme, "Shortcuts", Some("Shown for reference. Remapping is not available yet."), list))
        .into_any_element()
}

fn about(snap: &Snapshot, theme: &Theme, state: &Entity<AppState>) -> AnyElement {
    let link = |id: &'static str, label: &'static str, url: &'static str| {
        text_button(id, label, theme, false, move |cx| cx.open_url(url))
    };
    let reset: AnyElement = if snap.confirm_reset {
        let (yes, no) = (state.clone(), state.clone());
        div()
            .flex()
            .items_center()
            .gap_2()
            .child("Put every setting back to its default?")
            .child(text_button("reset-yes", "Reset", theme, true, move |cx| {
                yes.update(cx, |s, cx| {
                    s.confirm_reset = false;
                    s.reset_settings(cx)
                })
            }))
            .child(text_button("reset-no", "Cancel", theme, false, move |cx| {
                no.update(cx, |s, cx| {
                    s.confirm_reset = false;
                    cx.notify()
                })
            }))
            .into_any_element()
    } else {
        let ask = state.clone();
        text_button("reset-settings", "Reset settings", theme, false, move |cx| {
            ask.update(cx, |s, cx| {
                s.confirm_reset = true;
                cx.notify()
            })
        })
        .into_any_element()
    };
    div()
        .child(section(
            theme,
            "About",
            None,
            vec![
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pb_4()
                    .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child("sdterm"))
                    .child(
                        div()
                            .max_w(px(520.))
                            .text_color(theme.muted_foreground)
                            .child("A terminal workspace by SEM Devs: many terminals in one window, each with a name, a note and a color, plus live resource monitors you can pop out."),
                    )
                    .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(format!("Version {VERSION}")))
                    .into_any_element(),
            ],
        ))
        .child(section(
            theme,
            "References",
            None,
            vec![
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .py_2()
                    .child(link("l-web", "Website", WEBSITE))
                    .child(link("l-mail", "Email", EMAIL))
                    .child(link("l-git", "GitHub", GITHUB))
                    .child(link("l-rate", "Rate sdterm on Google ★★★★★", REVIEW))
                    .into_any_element(),
            ],
        ))
        .child(section(theme, "Settings", None, vec![div().py_2().child(reset).into_any_element()]))
        .into_any_element()
}
