//! Self-update.
//!
//! The release script publishes `latest-mac.json`, `latest-windows.json` and `latest-linux.json`
//! next to the installers. The app reads the one for its platform, compares versions, and on
//! macOS downloads the new app, checks its checksum and signature, swaps it in place and reopens.
//! Elsewhere it sends you to the download page.

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

pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
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
        "{}/latest-{}.json?t={}",
        base.trim_end_matches('/'),
        platform(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs()
    );
    let mut response = agent(Some(Duration::from_secs(20))).get(&url).call().context("could not reach the update server")?;
    let text = response.body_mut().read_to_string()?;
    let feed: Feed = serde_json::from_str(&text).context("the update feed is not readable")?;
    if !is_newer(&feed.version, CURRENT) {
        return Ok(None);
    }
    let file = feed
        .files
        .iter()
        .find(|f| f.arch == arch())
        .or_else(|| feed.files.first())
        .ok_or_else(|| anyhow!("the update feed has no file for this computer"))?;
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
    let path = dir.join("update.zip");
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
        let name: &'static str = Box::leak(format!("/latest-{}.json", platform()).into_boxed_str());
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
        let name: &'static str = Box::leak(format!("/latest-{}.json", platform()).into_boxed_str());
        let base = serve(vec![(name, feed.into_bytes()), ("/new.zip", payload)]);
        let offer = check(&base).unwrap().unwrap();
        let err = download(&offer, |_| {}).unwrap_err().to_string();
        assert!(err.contains("checksum"), "{err}");
    }

    #[test]
    fn no_update_when_the_feed_is_not_newer() {
        let feed = format!(r#"{{"version":"{CURRENT}","files":[]}}"#);
        let name: &'static str = Box::leak(format!("/latest-{}.json", platform()).into_boxed_str());
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
        let file = feed.files.iter().find(|f| f.arch == arch()).unwrap();
        assert_eq!(file.url, "b.zip");
    }
}
