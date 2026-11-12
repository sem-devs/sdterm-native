//! Self-update.
//!
//! The release publishes `latest-mac.json`, `latest-win.json` and `latest-linux.json` next to the
//! installers. The app reads the one for its system, compares versions, downloads the file it
//! names, checks its SHA-512 and installs it:
//!
//! * macOS: the zipped app is unpacked, its signature checked, and it replaces the running app.
//! * Windows (installed with the setup): the setup runs silently over the old install, then the
//!   new version starts.
//! * Linux AppImage: the AppImage file is replaced and started again.
//!
//! Everything else (a .deb, a .tar.gz or a plain zip) is sent to the download page.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha512};

pub const DEFAULT_FEED: &str = "https://55xxovadfasfy2ql.public.blob.vercel-storage.com/sdterm/native-updates";
pub const DOWNLOAD_PAGE: &str = "https://sem-devs.com/en/sdterm";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, notes: String },
    Downloading(u8),
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Offer {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub sha512: String,
}

#[derive(Deserialize)]
struct Feed {
    version: String,
    #[serde(default)]
    notes: String,
    files: Vec<FeedFile>,
}

#[derive(Deserialize)]
struct FeedFile {
    url: String,
    sha512: String,
    #[serde(default)]
    arch: String,
}

fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
}

/// The name of this system in the feed file: `latest-<name>.json`.
fn feed_system() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(windows) {
        "win"
    } else {
        "linux"
    }
}

fn feed_file_name() -> String {
    format!("latest-{}.json", feed_system())
}

/// The file in the feed for this computer: the one for its architecture, else the first.
fn pick_file(files: &[FeedFile]) -> Option<&FeedFile> {
    files.iter().find(|f| f.arch == arch()).or_else(|| files.first())
}

/// What the downloaded file is called on disk, from the extension of its address (update.exe,
/// update.AppImage, update.zip), so an installer is recognised as one.
fn download_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let ext = path
        .rsplit('/')
        .next()
        .and_then(|file| file.rsplit_once('.'))
        .map(|(_, ext)| ext)
        .filter(|ext| !ext.is_empty() && ext.len() <= 12 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    format!("update.{ext}")
}

/// Where the update files are. `SDTERM_UPDATE_URL` points it elsewhere, for testing.
pub fn feed_base() -> String {
    std::env::var("SDTERM_UPDATE_URL").unwrap_or_else(|_| DEFAULT_FEED.to_string())
}

/// Updating is for installed apps. A development build only updates when told where to look.
pub fn enabled() -> bool {
    std::env::var_os("SDTERM_UPDATE_URL").is_some() || !cfg!(debug_assertions)
}

/// Whether `candidate` is a newer version than `current`. A pre-release (1.2.0-beta) is older than
/// the release it leads to.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let (core, pre) = match v.split_once('-') {
            Some((core, _)) => (core, true),
            None => (v, false),
        };
        (core.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
    }
    let (mut a, a_pre) = parts(candidate);
    let (mut b, b_pre) = parts(current);
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => b_pre && !a_pre,
    }
}

fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(timeout).build().into()
}

/// Asks the feed whether there is a newer version. `None` means you are up to date.
pub fn check(base: &str) -> Result<Option<Offer>> {
    let url = format!(
        "{}/{}?t={}",
        base.trim_end_matches('/'),
        feed_file_name(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs()
    );
    let mut response = agent(Some(Duration::from_secs(20))).get(&url).call().context("could not reach the update server")?;
    let text = response.body_mut().read_to_string()?;
    let feed: Feed = serde_json::from_str(&text).context("the update feed is not readable")?;
    if !is_newer(&feed.version, CURRENT) {
        return Ok(None);
    }
    let file = pick_file(&feed.files).ok_or_else(|| anyhow!("the update feed has no file for this computer"))?;
    let file_url = if file.url.starts_with("http") {
        file.url.clone()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), file.url)
    };
    Ok(Some(Offer { version: feed.version, notes: feed.notes, url: file_url, sha512: file.sha512.clone() }))
}

/// Downloads the file, hashing as it arrives so a damaged or tampered file is never unpacked.
pub fn download(offer: &Offer, progress: impl Fn(u8)) -> Result<PathBuf> {
    let mut response = agent(None).get(&offer.url).call().context("could not download the update")?;
    let total = response.body().content_length().unwrap_or(0);
    // A folder of its own for each download, so two never step on each other.
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let dir = std::env::temp_dir().join(format!("sdterm-update-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(download_name(&offer.url));
    let mut out = std::fs::File::create(&path)?;
    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha512::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        out.write_all(&buffer[..n])?;
        done += n as u64;
        if total > 0 {
            progress((done * 100 / total).min(100) as u8);
        }
    }
    out.flush()?;
    let got = base64::engine::general_purpose::STANDARD.encode(hasher.finalize());
    if got != offer.sha512 {
        let _ = std::fs::remove_file(&path);
        bail!("the downloaded update does not match its checksum, so it was not installed");
    }
    Ok(path)
}

/// The app bundle the running program lives in (macOS).
fn current_bundle() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("sdterm is not running from an app bundle"))
}

/// Unpacks the new app, checks its signature, and arranges for it to replace this one after this
/// process has quit. The swap runs in a small script that outlives the app.
pub fn install_mac(zip: &Path) -> Result<()> {
    let old = current_bundle()?;
    let staging = zip.parent().ok_or_else(|| anyhow!("no folder"))?.join("unpacked");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let status = Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(zip).arg(&staging).status()?;
    if !status.success() {
        bail!("could not unpack the update");
    }
    let new = std::fs::read_dir(&staging)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or_else(|| anyhow!("the update has no app in it"))?;

    // The new app must carry a valid signature, whoever made it.
    let verify = Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&new).status()?;
    if !verify.success() {
        bail!("the downloaded app has an invalid signature, so it was not installed");
    }
    if let Some(parent) = old.parent() {
        let probe = parent.join(".sdterm-write-test");
        if std::fs::write(&probe, b"").is_err() {
            bail!("sdterm cannot replace itself in {}: move it to your Applications folder", parent.display());
        }
        let _ = std::fs::remove_file(probe);
    }

    let script = staging.join("swap.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/bash\nwhile kill -0 {pid} 2>/dev/null; do sleep 0.3; done\nBACKUP=\"{old}.previous\"\nrm -rf \"$BACKUP\"\nmv \"{old}\" \"$BACKUP\" && mv \"{new}\" \"{old}\" && rm -rf \"$BACKUP\"\nopen \"{old}\"\n",
            pid = std::process::id(),
            old = old.display(),
            new = new.display(),
        ),
    )?;
    // Backgrounded through a shell, so it keeps running after this process is gone.
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("nohup /bin/bash \"{}\" >/dev/null 2>&1 &", script.display()))
        .spawn()?;
    Ok(())
}

/// What the setup is started with by the app: silent, and "start the new version when done".
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_SETUP_ARGS: [&str; 2] = ["/S", "/UPDATE"];

/// Whether this program was put here by the Windows setup (it leaves `Uninstall.exe` beside it).
/// A program unpacked from the plain zip is not updated in place.
fn installed_by_setup(exe: &Path) -> bool {
    exe.parent().is_some_and(|dir| dir.join("Uninstall.exe").is_file())
}

/// The AppImage file this process was started from, if it is one. The AppImage runtime sets it.
fn appimage_path(var: Option<std::ffi::OsString>) -> Option<PathBuf> {
    var.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Whether the update can be installed by the app itself. Elsewhere (a .deb, a .tar.gz, a plain
/// zip, a development build) the app opens the download page instead.
pub fn can_install() -> bool {
    if cfg!(target_os = "macos") {
        true
    } else if cfg!(windows) {
        std::env::current_exe().is_ok_and(|exe| installed_by_setup(&exe))
    } else {
        appimage_path(std::env::var_os("APPIMAGE")).is_some()
    }
}

/// Installs the downloaded file for this system and arranges for the new version to open once this
/// process has quit. The caller quits the app when this returns `Ok`.
pub fn install(file: &Path) -> Result<()> {
    if cfg!(target_os = "macos") {
        install_mac(file)
    } else if cfg!(windows) {
        install_windows(file)
    } else {
        install_appimage(file)
    }
}

/// Runs the downloaded setup silently. It waits for this program to quit, installs over the old
/// version and starts the new one (see `scripts/windows/installer.nsi`).
#[cfg(windows)]
fn install_windows(setup: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let exe = std::env::current_exe()?;
    if !installed_by_setup(&exe) {
        bail!("this copy of sdterm was not installed with the setup, so it cannot update itself: download the new setup");
    }
    Command::new(setup)
        .args(WINDOWS_SETUP_ARGS)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .context("could not start the setup")?;
    Ok(())
}

#[cfg(not(windows))]
fn install_windows(_setup: &Path) -> Result<()> {
    bail!("the Windows setup can only be run on Windows")
}

/// Replaces `target` with the file `new`, atomically: the new file is first written beside the
/// target (same disk, so the final rename is one step) and renamed over it. A running AppImage
/// keeps working from the old file until it quits.
fn replace_file(new: &Path, target: &Path) -> Result<()> {
    let dir = target.parent().ok_or_else(|| anyhow!("the AppImage has no folder"))?;
    let name = target.file_name().ok_or_else(|| anyhow!("the AppImage has no name"))?.to_string_lossy().into_owned();
    let staged = dir.join(format!(".{name}.update"));
    let _ = std::fs::remove_file(&staged);
    std::fs::copy(new, &staged)
        .with_context(|| format!("sdterm cannot write in {}: move the AppImage to a folder you own", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    if let Err(err) = std::fs::rename(&staged, target) {
        let _ = std::fs::remove_file(&staged);
        return Err(err).context("could not replace the AppImage");
    }
    Ok(())
}

/// Replaces the AppImage this process runs from and opens the new one after this process quits.
fn install_appimage(new: &Path) -> Result<()> {
    let target = appimage_path(std::env::var_os("APPIMAGE"))
        .ok_or_else(|| anyhow!("sdterm is not running as an AppImage, so it cannot update itself: download the new version"))?;
    replace_file(new, &target)?;
    // Backgrounded through a shell, so it keeps running after this process is gone. The path goes
    // in through the environment, never into the script text.
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "(while kill -0 {} 2>/dev/null; do sleep 0.3; done; exec \"$SDTERM_RELAUNCH\") >/dev/null 2>&1 &",
            std::process::id()
        ))
        .env("SDTERM_RELAUNCH", &target)
        .spawn()
        .context("could not reopen sdterm")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions() {
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.1"));
        assert!(!is_newer("1.0.0", "1.0"));
    }

    #[test]
    fn a_prerelease_is_older_than_its_release() {
        assert!(is_newer("1.0.0", "1.0.0-beta"));
        assert!(!is_newer("1.0.0-beta", "1.0.0"));
        assert!(is_newer("1.0.1-beta", "1.0.0"));
    }

    /// A tiny web server for the tests: answers each request with the body for its path.
    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let path = path.split('?').next().unwrap().to_string();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                let body = files.iter().find(|(p, _)| *p == path).map(|(_, b)| b.clone());
                let (status, body) = match body {
                    Some(b) => ("200 OK", b),
                    None => ("404 Not Found", Vec::new()),
                };
                let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        base
    }

    fn sha(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    }

    #[test]
    fn finds_a_newer_version_on_a_server_and_downloads_it_checked() {
        let payload = b"pretend this is a zip".to_vec();
        let feed = format!(
            r#"{{"version":"99.0.0","notes":"n","files":[{{"arch":"{}","url":"new.zip","sha512":"{}"}}]}}"#,
            arch(),
            sha(&payload)
        );
        let name: &'static str = Box::leak(format!("/{}", feed_file_name()).into_boxed_str());
        let base = serve(vec![(name, feed.into_bytes()), ("/new.zip", payload.clone())]);

        let offer = check(&base).unwrap().expect("99.0.0 is newer");
        assert_eq!(offer.version, "99.0.0");
        assert!(offer.url.ends_with("/new.zip"));

        let seen = std::sync::Mutex::new(Vec::new());
        let path = download(&offer, |n| seen.lock().unwrap().push(n)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), payload);
        assert_eq!(*seen.lock().unwrap().last().unwrap(), 100);
    }

    #[test]
    fn a_file_that_does_not_match_its_checksum_is_refused_and_removed() {
        let payload = b"the real file".to_vec();
        let feed = format!(
            r#"{{"version":"99.0.0","files":[{{"arch":"{}","url":"new.zip","sha512":"{}"}}]}}"#,
            arch(),
            sha(b"something else entirely")
        );
        let name: &'static str = Box::leak(format!("/{}", feed_file_name()).into_boxed_str());
        let base = serve(vec![(name, feed.into_bytes()), ("/new.zip", payload)]);
        let offer = check(&base).unwrap().unwrap();
        let err = download(&offer, |_| {}).unwrap_err().to_string();
        assert!(err.contains("checksum"), "{err}");
    }

    #[test]
    fn no_update_when_the_feed_is_not_newer() {
        let feed = format!(r#"{{"version":"{CURRENT}","files":[]}}"#);
        let name: &'static str = Box::leak(format!("/{}", feed_file_name()).into_boxed_str());
        let base = serve(vec![(name, feed.into_bytes())]);
        assert!(check(&base).unwrap().is_none());
    }

    #[test]
    fn an_unreachable_server_is_an_error_not_a_hang() {
        let started = std::time::Instant::now();
        assert!(check("http://127.0.0.1:1").is_err());
        assert!(started.elapsed() < Duration::from_secs(25));
    }

    #[test]
    fn reads_a_feed_and_picks_the_file_for_this_computer() {
        let json = format!(
            r#"{{"version":"9.9.9","notes":"x","files":[
              {{"arch":"other","url":"a.zip","sha512":"A"}},
              {{"arch":"{}","url":"b.zip","sha512":"B"}}]}}"#,
            arch()
        );
        let feed: Feed = serde_json::from_str(&json).unwrap();
        let file = pick_file(&feed.files).unwrap();
        assert_eq!(file.url, "b.zip");
    }

    #[test]
    fn without_a_file_for_this_architecture_the_first_one_is_used() {
        let json = r#"{"version":"9.9.9","files":[{"arch":"other","url":"a.exe","sha512":"A"}]}"#;
        let feed: Feed = serde_json::from_str(json).unwrap();
        assert_eq!(pick_file(&feed.files).unwrap().url, "a.exe");
        assert!(pick_file(&[]).is_none());
    }

    #[test]
    fn each_system_reads_its_own_feed_file() {
        let name = feed_file_name();
        if cfg!(target_os = "macos") {
            assert_eq!(name, "latest-mac.json");
        } else if cfg!(windows) {
            assert_eq!(name, "latest-win.json");
        } else {
            assert_eq!(name, "latest-linux.json");
        }
    }

    #[test]
    fn the_downloaded_file_is_named_after_its_extension() {
        assert_eq!(download_name("https://x.test/native-updates/sdterm-native-1.0.0-mac-arm64.zip"), "update.zip");
        assert_eq!(download_name("https://x.test/a/sdterm-native-1.0.0-win-x64-setup.exe"), "update.exe");
        assert_eq!(download_name("https://x.test/a/sdterm-native-1.0.0-linux-x86_64.AppImage?token=1"), "update.AppImage");
        assert_eq!(download_name("https://x.test/a/noextension"), "update.bin");
        assert_eq!(download_name("https://x.test/a.b/noextension"), "update.bin");
    }

    #[test]
    fn a_setup_and_an_appimage_are_downloaded_checked_and_named_for_what_they_are() {
        for file in ["setup.exe", "app.AppImage"] {
            let payload = format!("pretend this is {file}").into_bytes();
            let feed = format!(
                r#"{{"version":"99.0.0","files":[{{"arch":"{}","url":"{file}","sha512":"{}"}}]}}"#,
                arch(),
                sha(&payload)
            );
            let name: &'static str = Box::leak(format!("/{}", feed_file_name()).into_boxed_str());
            let url_path: &'static str = Box::leak(format!("/{file}").into_boxed_str());
            let base = serve(vec![(name, feed.into_bytes()), (url_path, payload.clone())]);
            let offer = check(&base).unwrap().unwrap();
            let path = download(&offer, |_| {}).unwrap();
            assert_eq!(path.file_name().unwrap().to_string_lossy(), format!("update.{}", file.rsplit('.').next().unwrap()));
            assert_eq!(std::fs::read(&path).unwrap(), payload);
        }
    }

    #[test]
    fn the_setup_is_started_silently_and_asked_to_reopen_the_app() {
        assert_eq!(WINDOWS_SETUP_ARGS, ["/S", "/UPDATE"]);
    }

    #[test]
    fn only_a_program_put_there_by_the_setup_updates_itself_in_place() {
        let dir = std::env::temp_dir().join(format!("sdterm-installed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("sdterm-native.exe");
        std::fs::write(&exe, b"x").unwrap();
        assert!(!installed_by_setup(&exe), "a plain unzip has no uninstaller");
        std::fs::write(dir.join("Uninstall.exe"), b"x").unwrap();
        assert!(installed_by_setup(&exe));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_appimage_path_comes_from_the_environment_variable() {
        assert_eq!(appimage_path(None), None);
        assert_eq!(appimage_path(Some("".into())), None);
        assert_eq!(appimage_path(Some("/home/me/sdterm.AppImage".into())), Some(PathBuf::from("/home/me/sdterm.AppImage")));
    }

    #[cfg(unix)]
    #[test]
    fn replacing_an_appimage_swaps_the_file_in_one_step_and_leaves_nothing_behind() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sdterm-appimage-{}-{}", std::process::id(), fastrand::u32(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("sdterm-native.AppImage");
        let new = dir.join("update.AppImage");
        std::fs::write(&target, b"old version").unwrap();
        std::fs::write(&new, b"new version").unwrap();
        // A file someone else is still running from keeps its contents: simulate by holding it open.
        let still_running = std::fs::File::open(&target).unwrap();

        replace_file(&new, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new version");
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        let mut old = String::new();
        let mut still_running = still_running;
        still_running.read_to_string(&mut old).unwrap();
        assert_eq!(old, "old version");
        let mut names: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["sdterm-native.AppImage", "update.AppImage"], "no staging file is left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_written_gives_a_clear_error_and_keeps_the_old_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sdterm-readonly-{}-{}", std::process::id(), fastrand::u32(..)));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("sdterm-native.AppImage");
        let new = std::env::temp_dir().join(format!("sdterm-new-{}-{}", std::process::id(), fastrand::u32(..)));
        std::fs::write(&target, b"old version").unwrap();
        std::fs::write(&new, b"new version").unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = replace_file(&new, &target);

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Running as root ignores the permission; otherwise it must fail without touching the file.
        if result.is_err() {
            assert!(result.unwrap_err().to_string().contains("cannot write"));
            assert_eq!(std::fs::read(&target).unwrap(), b"old version");
        }
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&new);
    }
}
