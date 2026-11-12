//! sdterm native: a terminal workspace for macOS, Windows and Linux, drawn on the GPU.

mod layout;
mod metrics;
mod model;
mod monitors;
mod resources;
mod session;
mod state;
mod store;
mod text_input;
mod theme;
mod ui;

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowAppearance,
    WindowBounds, WindowOptions, point, px, size,
};

use crate::state::AppState;
use crate::theme::Theme;
use crate::metrics::WidgetKind;
use crate::ui::{
    CloseTerminal, NewTerminal, NewWorkspace, OpenSettings, PopOutCpu, PopOutCpuRam, PopOutDisk,
    PopOutNet, PopOutProcesses, PopOutRam, PopOutSummary, Quit, Root, ToggleSidebar,
};

fn main() {
    Application::new().run(|cx: &mut App| {
        let mode = store::load().data.settings.appearance.theme;
        let system_dark =
            matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
        cx.set_global(Theme::for_mode(mode, system_dark));

        let state = cx.new(AppState::new);

        // Cmd on a Mac. Elsewhere plain Ctrl+T/W/N belong to the shell, so the shortcuts use
        // Ctrl+Shift there.
        let m = |key: &str| if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-shift-{key}") };
        cx.bind_keys([
            KeyBinding::new(&m("t"), NewTerminal, None),
            KeyBinding::new(&m("w"), CloseTerminal, None),
            KeyBinding::new(&m("n"), NewWorkspace, None),
            KeyBinding::new(&m("b"), ToggleSidebar, None),
            KeyBinding::new(&m(","), OpenSettings, None),
            KeyBinding::new(&m("q"), Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
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
            on_quit.update(cx, |state, _| state.shutdown());
            async {}
        })
        .detach();

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("sdterm".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.), px(14.))),
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1280.), px(800.)),
                cx,
            ))),
            ..Default::default()
        };
        let root_state = state.clone();
        cx.open_window(options, move |_window, cx| cx.new(|cx| Root::new(root_state, cx)))
            .expect("could not open the window");
        cx.activate(true);
    });
}
