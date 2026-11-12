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

1. Bump `version` in `Cargo.toml` (three numbers, like `0.2.0`: the Windows installer needs that).
2. On a Mac with the Developer ID certificate and the notary profile: `scripts/package-mac.sh all`
   builds, signs and notarizes the apps and the DMGs and writes `latest-mac.json` into `dist/<version>/`.
3. The "Build" workflow builds the Linux and Windows downloads (run it by hand: Actions, Build, or
   `gh workflow run build.yml --repo sem-devs/sdterm-native`; a pushed tag like `v0.2.0` runs it too).
   Fetch them next to the Mac files: `gh run download <run id> --repo sem-devs/sdterm-native -D dist/<version>`.
4. Publish for installed apps: `cd scripts && npm install`, then
   `BLOB_READ_WRITE_TOKEN=... node scripts/publish-update.mjs dist/<version>`. It checks every feed
   against the files it names, uploads the files and then the three feeds.
5. Put the installers on the website (`scripts/upload-sdterm-downloads.mjs` in the website repo) and
   create the GitHub release with the same files.

What the workflow produces (in `dist/`, as two artifacts):

| File                                           | What it is                                                  |
| ---------------------------------------------- | ----------------------------------------------------------- |
| `sdterm-native-<v>-win-x64-setup.exe`          | NSIS installer, per user, `/S` silent, `/UPDATE` for updates |
| `sdterm-native-<v>-win-x64.zip`                | the plain program, no installer, does not self-update       |
| `sdterm-native-<v>-linux-x86_64.AppImage`      | one file, self-updating                                     |
| `sdterm-native-<v>-linux-amd64.deb`            | Debian and Ubuntu package                                   |
| `sdterm-native-<v>-linux-x86_64.tar.gz`        | the program with its `resources` folder                     |
| `latest-win.json`, `latest-linux.json`         | the update feeds (SHA-512 in base64)                        |
| `SHA256SUMS-win.txt`, `SHA256SUMS-linux.txt`   | checksums                                                   |

Nothing for Windows is code signed: we have no certificate, so SmartScreen warns on the first run.

The updater reads `latest-mac.json`, `latest-win.json` or `latest-linux.json` (the one for its system) from
`https://55xxovadfasfy2ql.public.blob.vercel-storage.com/sdterm/native-updates` (override with
`SDTERM_UPDATE_URL`). Each is `{ "version", "notes", "releaseDate", "files": [ { "arch": "x64", "url":
"<file name>", "sha512": "<base64>", "size": <bytes> } ] }`, written by `scripts/make-feed.mjs` (or
`scripts/package-mac.sh`). What happens next depends on the install:

- macOS: it downloads the zip, checks its SHA-512 and the app's signature, swaps the app and reopens it.
- Windows, installed with the setup: it downloads the setup, checks its SHA-512, runs it with
  `/S /UPDATE` and quits. The setup waits for the old program to be free, installs over it and starts
  the new version. A copy run from the plain zip opens the download page instead.
- Linux AppImage (the `APPIMAGE` variable is set): it downloads the AppImage, checks its SHA-512,
  replaces the file in one rename and starts the new one after quitting. A `.deb` or `.tar.gz` install
  opens the download page.

In code: `updater::can_install()` says whether the update can be installed in place and
`updater::install(file)` does it for the running system.
