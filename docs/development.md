# Development notes

## How it is organized

```
src/state.rs        the one shared state: workspaces, terminals, settings, windows, updates
src/session.rs      starting a shell and the terminal view that shows it
src/layout.rs       how a workspace arranges its terminals, as a tree of splits
src/model.rs        the data, in the same JSON layout as the Electron app's sdterm.json
src/store.rs        reading and writing the data file (atomic saves, import from Electron)
src/ui/             the views: sidebar, panes, settings, charts, menus, form controls
src/monitors.rs     the pop-out windows
src/resources.rs    the sampler behind the monitors
src/updater.rs      self-update
src/text_input.rs   a text field that works with dictation and accents
crates/gpui-terminal  the terminal view: emulation by alacritty_terminal, painting, input, mouse
```

The main idea: **shells belong to the app, not to a window.** Every terminal's shell lives in
`AppState`, so switching workspace hides a terminal without touching its shell, and a workspace can
move to another window with its processes.

Views never change data themselves. They read a snapshot of the state and call methods on it, so every
change goes through the same checks and the same saving.

## Testing

`cargo test` covers the logic: the data model against a real Electron file, layouts, the store,
resource numbers, the update feed and download, the text field's cursor moves. The views are not under
test: GPUI has no way to see a window from the command line, so they are checked by hand.

On macOS, run the app from a bundle (`scripts/bundle-dev.sh`, then `open -n "target/debug/bundle/sdterm native.app"`).
A bare program has no bundle identifier, and then macOS dictation never starts recording. The debug
build prints a trace of the text input protocol (`[ime]` lines) to stderr.

## Releasing

1. Bump `version` in `Cargo.toml`.
2. On a Mac with the Developer ID certificate and the notary profile: `scripts/package-mac.sh all`
   builds, signs and notarizes the apps and the DMGs and writes `latest-mac.json` into `dist/<version>/`.
3. The "Build" workflow builds the Linux and Windows downloads (run it by hand or push a tag).
4. Publish for installed apps: `cd scripts && npm install`, then
   `BLOB_READ_WRITE_TOKEN=... node scripts/publish-update.mjs dist/<version>`.
5. Create the GitHub release with the same files.

The updater reads `latest-mac.json`, `latest-windows.json` and `latest-linux.json` from
`https://55xxovadfasfy2ql.public.blob.vercel-storage.com/sdterm/native-updates` (override with
`SDTERM_UPDATE_URL`). On macOS it downloads the zip, checks its SHA-512 and the app's signature, swaps
the app and reopens it. On Windows and Linux it opens the download page.
