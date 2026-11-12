//! Reading and writing the data file.
//!
//! The native app keeps its own file next to the Electron app's one, in the same folder, so both
//! can be installed at once. The first time it runs it imports the Electron file (read only); after
//! that it only touches its own. Saving is atomic: a crash half way never leaves a broken file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{Settings, StoreData};

pub const NATIVE_FILE: &str = "sdterm-native.json";
pub const ELECTRON_FILE: &str = "sdterm.json";

/// Where the app keeps its data: the same folder the Electron app uses on each platform.
pub fn data_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sdterm")
}

pub struct Loaded {
    pub data: StoreData,
    /// What happened, for the log: a fresh start, an import, or a damaged file set aside.
    pub note: String,
}

pub fn load() -> Loaded {
    load_from(&data_dir())
}

fn read(path: &Path) -> Result<StoreData, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut data: StoreData = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    data.settings = std::mem::take(&mut data.settings).normalized();
    Ok(data)
}

pub fn load_from(dir: &Path) -> Loaded {
    let native = dir.join(NATIVE_FILE);
    if native.exists() {
        return match read(&native) {
            Ok(data) => Loaded { data, note: "loaded".into() },
            Err(err) => {
                // Keep the damaged file for the user instead of overwriting it.
                let broken = dir.join(format!("{NATIVE_FILE}.broken"));
                let _ = fs::rename(&native, &broken);
                Loaded {
                    data: StoreData::default(),
                    note: format!("the data file was damaged ({err}); kept as {}", broken.display()),
                }
            }
        };
    }
    let electron = dir.join(ELECTRON_FILE);
    if electron.exists() {
        if let Ok(data) = read(&electron) {
            return Loaded { data, note: "imported from the Electron app".into() };
        }
    }
    Loaded {
        data: StoreData { settings: Settings::default(), ..StoreData::default() },
        note: "fresh start".into(),
    }
}

pub fn save(data: &StoreData) -> io::Result<()> {
    save_to(&data_dir(), data)
}

pub fn save_to(dir: &Path, data: &StoreData) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{NATIVE_FILE}.tmp"));
    fs::write(&tmp, json)?;
    fs::rename(tmp, dir.join(NATIVE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PersistedWorkspaces, Workspace};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sdterm-native-test-{name}-{}", fastrand::u32(..)));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The real file on this machine, if there is one: it must load (read only).
    #[test]
    fn reads_the_real_electron_file_if_there_is_one() {
        let path = data_dir().join(ELECTRON_FILE);
        if path.exists() {
            let data = read(&path).expect("the Electron app's file must be readable");
            assert!(data.settings.terminal.font_size >= 9.0);
        }
    }

    #[test]
    fn a_fresh_start_has_default_settings_and_no_workspaces() {
        let dir = scratch("fresh");
        let loaded = load_from(&dir);
        assert_eq!(loaded.note, "fresh start");
        assert!(loaded.data.workspaces.is_none());
        assert_eq!(loaded.data.settings, Settings::default());
    }

    #[test]
    fn saves_and_loads_back() {
        let dir = scratch("roundtrip");
        let mut data = StoreData::default();
        data.workspaces = Some(PersistedWorkspaces {
            workspaces: vec![Workspace::new("Project", "main")],
            ..Default::default()
        });
        save_to(&dir, &data).unwrap();
        assert!(!dir.join(format!("{NATIVE_FILE}.tmp")).exists(), "no temp file left behind");
        let loaded = load_from(&dir);
        assert_eq!(loaded.note, "loaded");
        assert_eq!(loaded.data, data);
    }

    #[test]
    fn imports_the_electron_file_once_and_never_writes_to_it() {
        let dir = scratch("import");
        let original = r#"{"workspaces": {"workspaces": [{"id": "w", "name": "From Electron"}]}}"#;
        fs::write(dir.join(ELECTRON_FILE), original).unwrap();
        let loaded = load_from(&dir);
        assert_eq!(loaded.note, "imported from the Electron app");
        assert_eq!(loaded.data.workspaces.as_ref().unwrap().workspaces[0].name, "From Electron");
        save_to(&dir, &loaded.data).unwrap();
        assert_eq!(fs::read_to_string(dir.join(ELECTRON_FILE)).unwrap(), original);
        // From now on its own file wins.
        assert_eq!(load_from(&dir).note, "loaded");
    }

    #[test]
    fn a_damaged_file_is_kept_aside_not_overwritten() {
        let dir = scratch("broken");
        fs::write(dir.join(NATIVE_FILE), "{ not json").unwrap();
        let loaded = load_from(&dir);
        assert!(loaded.note.starts_with("the data file was damaged"));
        assert!(dir.join(format!("{NATIVE_FILE}.broken")).exists());
        assert!(loaded.data.workspaces.is_none());
    }
}
