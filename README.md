# sdterm native

A native rewrite of [sdterm](https://github.com/sem-devs/sdterm) in Rust, for macOS, Windows and Linux.
No Electron, no web view: the window is drawn on the GPU with [GPUI](https://gpui.rs), the terminal
is [alacritty_terminal](https://docs.rs/alacritty_terminal), shells run through
[portable-pty](https://docs.rs/portable-pty) (ConPTY on Windows).

Why: the Electron app works, but it weighs about 135 MB to install and 350 MB of RAM with a few
terminals open. A native app should be a fraction of that and start faster.

## Status: spike

What runs today (`cargo run`): one window with four real login shells in a 2x2 grid, each with a
name and a color. The shells get the same clean environment as the Electron app (no leftovers from
whatever launched it, `TERM_PROGRAM=sdterm`, a UTF-8 `LANG`).

Not there yet, in the order they matter:

1. **Typing like Terminal.app.** The terminal view we build on does not register a text input
   handler, so macOS dictation, the emoji picker and dead keys do not work, and Option always sends
   ESC. It also has no mouse selection and no scrollback navigation. We will bring the view into this
   repo and finish it. Dictation is the make-or-break item: if it cannot be done natively, stop here.
2. Workspaces, names, notes, colors, layouts and drag and drop, reading the same `sdterm.json` as
   the Electron app so nobody loses anything.
3. Multiple windows, park and launch, resource monitors and processes window, settings.
4. Auto update, signed and notarized installers (Developer ID is already set up).
5. zsh suggestions and Tab completion (the shell files from the Electron app carry over).

## Build

```
cargo run
```

On macOS GPUI needs the Metal toolchain: `xcodebuild -downloadComponent MetalToolchain` once.

## Licenses

MIT. Dependencies: gpui 0.2.2 (Apache-2.0), gpui-terminal 0.1.0 (MIT or Apache-2.0),
alacritty_terminal (Apache-2.0), portable-pty (MIT).
