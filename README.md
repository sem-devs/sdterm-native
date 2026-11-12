# sdterm native

Many terminals in one window, each with a name, a note and a color, so you always know what each
one was for. Your projects come back when you reopen it. Made by [SEM Devs](https://sem-devs.com),
free and open source.

This is the native version of [sdterm](https://github.com/sem-devs/sdterm), written in Rust for
macOS, Windows and Linux. There is no Electron and no web view: the window is drawn on the GPU, and
the program is a few megabytes instead of a few hundred.

## What it does

- **A name, a note and a color for every terminal.** Write what it is for and where you left off.
  You see it at a glance, in the title bar and in the sidebar.
- **Your projects come back.** Quit and reopen: every workspace, terminal, name and note is there,
  and each terminal starts again in the folder it was in.
- **Park what you are not working on.** A parked project stops its shells and costs nothing. Launch
  it again with one click.
- **More than one window.** Spread projects over several windows, pick a layout (columns, rows,
  grid, quarters, one big and a few small), drag a terminal's title onto another to swap them and drag
  the line between them to resize.
- **Live monitors.** CPU, RAM, disk and network, plus a list of the busiest processes. Pop any of
  them out as a small window.
- **A normal terminal.** It runs your own shell with your own settings. Keys behave like
  Terminal.app on a Mac, dictation works, you can scroll back, select and copy, and open any
  terminal's folder in Finder or Explorer.
- **Suggestions as you type.** With zsh, commands you ran before show up in grey and the right arrow
  accepts them; Tab completes git branches, npm scripts and more. Your own shell files are not touched.
- **Updates itself.** A small banner tells you when a new version is out.

It reads the data file of the Electron version, so moving over does not lose anything. It keeps its
own file next to it and never writes to the other one.

## Install

Downloads are on the [SEM Devs website](https://sem-devs.com/sdterm).

| System            | Download                                                                   |
| ----------------- | -------------------------------------------------------------------------- |
| macOS 12 or later | `.dmg`, Apple Silicon and Intel, available now                             |
| Windows 10 and 11 | `-win-x64-setup.exe` installer, or a plain `.zip` (x64), preview           |
| Linux (x86_64)    | `.AppImage`, `.deb` or `.tar.gz`, preview                                  |

The macOS app is signed by SEM Devs and notarized by Apple. The Windows and Linux builds are made
by the workflow in `.github/workflows/build.yml`. They are a preview: they build and pass the tests
on Windows and Linux runners, but have had little use on real machines yet.

**Windows.** The setup installs for your user only (no administrator prompt) into
`%LOCALAPPDATA%\Programs\sdterm native`, adds a Start menu shortcut and an entry in "Apps and
features". The files are not code signed (we have no Windows certificate), so SmartScreen may warn the
first time: click "More info", then "Run anyway". The plain `.zip` runs from any folder and does not
update itself.

**Linux.** The AppImage is one file: make it executable and run it (`chmod +x` then
`./sdterm-native-*.AppImage`; if it does not start, FUSE is missing on your system: run it with
`--appimage-extract-and-run`). It updates itself in place. The `.deb` (`sudo apt install ./file.deb`)
and the `.tar.gz` send you to the download page when a new version is out. The program needs the
usual desktop libraries (Wayland or X11, Vulkan, fontconfig, xkbcommon), which any desktop install has.

## Build it yourself

You need Rust (stable). On macOS also Xcode and its Metal toolchain
(`xcodebuild -downloadComponent MetalToolchain`, once). On Linux see the list of packages in
`.github/workflows/build.yml`.

```
cargo run --release
cargo test
```

On a Mac, `scripts/bundle-dev.sh` wraps a build in an app bundle. Use it for anything that depends on
being a real app: dictation does not start for a bare program. Developer notes are in
[docs/development.md](docs/development.md).

## Where things are

```
src/            the app: state, layouts, the terminal sessions, the views
crates/         gpui-terminal, the terminal view we build on, taken in and extended
resources/      the logo, the font, the zsh files, the macOS bundle files
scripts/        bundling and release scripts
```

## Why we built it

We work on many projects at the same time. After a few days we could not tell which terminal was for
what, and every restart meant opening everything again by hand. We wanted one window that remembers
what each terminal is for, where it was, and which projects we are not touching this week. We built
the first version with Electron, then rewrote it so it is light enough to leave open all day.

## License

MIT, see [LICENSE](LICENSE). Built on [GPUI](https://gpui.rs) (Apache-2.0),
[alacritty_terminal](https://github.com/alacritty/alacritty) (Apache-2.0),
[portable-pty](https://github.com/wez/wezterm) (MIT) and
[gpui-terminal](https://github.com/zortax/gpui-terminal) (MIT or Apache-2.0). The terminal font is
[Geist Mono](https://github.com/vercel/geist-font) (SIL Open Font License) and command suggestions come
from [zsh-autosuggestions](https://github.com/zsh-users/zsh-autosuggestions) (MIT). The SEM Devs name
and logo are not covered by the license.
