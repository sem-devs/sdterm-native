//! sdterm native: a terminal workspace for macOS, Windows and Linux, drawn on the GPU.

mod assets;
mod layout;
mod metrics;
mod model;
mod monitors;
mod resources;
mod session;
mod state;
mod store;
mod text_input;
mod updater;
mod theme;
mod ui;

use gpui::{App, AppContext, Application, KeyBinding, Menu, MenuItem, WindowAppearance};

use crate::state::AppState;
use crate::theme::Theme;
use crate::metrics::WidgetKind;
use crate::ui::{
    CloseTerminal, NewTerminal, NewWindow, NewWorkspace, OpenSettings, PopOutCpu, PopOutCpuRam, PopOutDisk,
    PopOutNet, PopOutProcesses, PopOutRam, PopOutSummary, Quit, ToggleSidebar,
};

fn main() {
    Application::new().with_assets(crate::assets::Assets).run(|cx: &mut App| {
        crate::assets::load_fonts(cx);
        let mode = store::load().data.settings.appearance.theme;
        let system_dark =
            matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
        cx.set_global(Theme::for_mode(mode, system_dark));

        let state = cx.new(AppState::new);
        state.update(cx, |s, cx| s.schedule_update_checks(cx));

        // Cmd on a Mac. Elsewhere plain Ctrl+T/W/N belong to the shell, so the shortcuts use
        // Ctrl+Shift there.
        let m = |key: &str| if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-shift-{key}") };
        cx.bind_keys([
            KeyBinding::new(&m("t"), NewTerminal, None),
            KeyBinding::new(&m("w"), CloseTerminal, None),
            KeyBinding::new(&m("n"), NewWorkspace, None),
            KeyBinding::new(if cfg!(target_os = "macos") { "cmd-shift-n" } else { "ctrl-alt-n" }, NewWindow, None),
            KeyBinding::new(&m("b"), ToggleSidebar, None),
            KeyBinding::new(&m(","), OpenSettings, None),
            KeyBinding::new(&m("q"), Quit, None),
        ]);
        let quit_state = state.clone();
        cx.on_action(move |_: &Quit, cx| {
            quit_state.update(cx, |s, _| s.quitting = true);
            cx.quit()
        });
        let window_state = state.clone();
        cx.on_action(move |_: &NewWindow, cx| {
            let id = window_state.update(cx, |s, cx| {
                let id = s.new_window_id();
                s.register_window(&id);
                s.add_workspace_in(&id, cx);
                id
            });
            crate::ui::open_app_window(cx, &window_state, id);
        });
        macro_rules! pop_out {
            ($($action:ident => $kind:expr),* $(,)?) => {$(
                let st = state.clone();
                cx.on_action(move |_: &$action, cx| crate::monitors::open(cx, $kind, &st));
            )*};
        }
        pop_out!(
            PopOutSummary => WidgetKind::Summary,
            PopOutCpu => WidgetKind::Cpu,
            PopOutCpuRam => WidgetKind::CpuRam,
            PopOutRam => WidgetKind::Ram,
            PopOutDisk => WidgetKind::Disk,
            PopOutNet => WidgetKind::Net,
            PopOutProcesses => WidgetKind::Processes,
        );
        cx.set_menus(vec![
            Menu {
                name: "sdterm".into(),
                items: vec![
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit sdterm", Quit),
                ],
            },
            Menu {
                name: "Shell".into(),
                items: vec![
                    MenuItem::action("New Terminal", NewTerminal),
                    MenuItem::action("Close Terminal", CloseTerminal),
                    MenuItem::separator(),
                    MenuItem::action("New Workspace", NewWorkspace),
                    MenuItem::action("New Window", NewWindow),
                ],
            },
            Menu {
                name: "Resources".into(),
                items: vec![
                    MenuItem::action("Pop Out Resources Summary", PopOutSummary),
                    MenuItem::separator(),
                    MenuItem::action("Pop Out CPU", PopOutCpu),
                    MenuItem::action("Pop Out CPU and RAM", PopOutCpuRam),
                    MenuItem::action("Pop Out RAM", PopOutRam),
                    MenuItem::action("Pop Out Disk I/O", PopOutDisk),
                    MenuItem::action("Pop Out Network I/O", PopOutNet),
                    MenuItem::separator(),
                    MenuItem::action("Processes", PopOutProcesses),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![MenuItem::action("Toggle Sidebar", ToggleSidebar)],
            },
        ]);

        // Before quitting: remember where each shell is, stop them, write the file.
        let on_quit = state.clone();
        cx.on_app_quit(move |cx| {
            on_quit.update(cx, |state, cx| state.shutdown(cx));
            async {}
        })
        .detach();

        // One window for each window the saved workspaces belong to.
        let ids = state.read(cx).window_ids();
        for id in ids {
            crate::ui::open_app_window(cx, &state, id);
        }
        cx.activate(true);
    });
}
