use crate::{
    Result,
    cloud::{CloudSession, DiscoveredDevice, random_hex},
    problem,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Settings {
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub ids: BTreeMap<String, u32>,
    #[serde(default)]
    pub devices: Vec<DiscoveredDevice>,
}
#[derive(Clone)]
pub(crate) struct Record {
    pub cloud_id: String,
    pub local_id: u32,
    pub label: String,
    pub mac: String,
}
pub(crate) struct Store {
    pub directory: PathBuf,
    pub client_id: String,
}
impl Store {
    pub fn open(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)
            .map_err(|_| problem("Cannot create the controller data directory."))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|_| problem("Cannot protect the controller data directory."))?;
        }
        let mut s = Self {
            directory,
            client_id: String::new(),
        };
        let _lock = s.lock()?;
        let path = s.directory.join("client.json");
        s.client_id = if path.exists() {
            let v: serde_json::Value = read_json(&path)?;
            v.get("device_id")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| problem("Invalid client identity file."))?
                .to_owned()
        } else {
            let id = random_hex(8);
            atomic_json(&path, &serde_json::json!({"device_id":id}))?;
            id
        };
        s.save(&s.settings()?)?;
        Ok(s)
    }
    pub fn lock(&self) -> Result<File> {
        let file = private_file(&self.directory.join("controller.lock"), false)?;
        file.lock_exclusive()
            .map_err(|_| problem("Could not lock controller data."))?;
        Ok(file)
    }
    pub async fn lock_async(&self) -> Result<File> {
        let path = self.directory.join("controller.lock");
        tokio::task::spawn_blocking(move || {
            let file = private_file(&path, false)?;
            file.lock_exclusive()
                .map_err(|_| problem("Could not lock controller data."))?;
            Ok(file)
        })
        .await
        .map_err(|_| problem("Could not acquire controller data lock."))?
    }
    pub fn settings(&self) -> Result<Settings> {
        let path = self.directory.join("settings.json");
        if path.exists() {
            read_json(&path)
        } else {
            Ok(Settings::default())
        }
    }
    pub fn save(&self, s: &Settings) -> Result<()> {
        atomic_json(&self.directory.join("settings.json"), s)
    }
    pub fn records(&self) -> Result<Vec<Record>> {
        let s = self.settings()?;
        let mut records = Vec::new();
        for device in &s.devices {
            let local_id = s.ids.get(&device.id).copied().ok_or_else(|| {
                problem(
                    "A discovered device has no local ID. Sign in again to refresh the inventory.",
                )
            })?;
            if local_id == 0 || records.iter().any(|r: &Record| r.local_id == local_id) {
                return Err(problem("Device IDs must be unique positive integers."));
            }
            records.push(Record {
                cloud_id: device.id.clone(),
                local_id,
                label: s
                    .labels
                    .get(&device.id)
                    .cloned()
                    .unwrap_or_else(|| device.name.clone()),
                mac: device.mac.clone(),
            });
        }
        Ok(records)
    }
    pub fn resolve(&self, selector: &str) -> Result<Record> {
        let records = self.records()?;
        let selector = selector.trim();
        if let Ok(id) = selector.parse::<u32>()
            && let Some(r) = records.iter().find(|r| r.local_id == id)
        {
            return Ok(r.clone());
        }
        if let Some(r) = records.iter().find(|r| r.cloud_id == selector) {
            return Ok(r.clone());
        }
        let matches = records
            .into_iter()
            .filter(|r| r.label.to_lowercase() == selector.to_lowercase())
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [r] => Ok(r.clone()),
            [] => Err(problem(
                "Unknown device. Run 'ac-man list' to see names and IDs.",
            )),
            _ => Err(problem("That room name is ambiguous. Use its integer ID.")),
        }
    }
    pub fn rename(&self, selector: &str, label: &str) -> Result<()> {
        let label = label.trim();
        if label.is_empty() || label.chars().count() > 60 || label.chars().any(char::is_control) {
            return Err(problem("Use a room name between 1 and 60 characters."));
        }
        let r = self.resolve(selector)?;
        if self.records()?.iter().any(|other| {
            other.cloud_id != r.cloud_id && other.label.to_lowercase() == label.to_lowercase()
        }) {
            return Err(problem("Another AC already uses that room name."));
        }
        let mut s = self.settings()?;
        s.labels.insert(r.cloud_id, label.into());
        self.save(&s)
    }
    pub fn assign_id(&self, selector: &str, id: u32) -> Result<()> {
        if id == 0 {
            return Err(problem("Device IDs must be positive integers."));
        }
        let r = self.resolve(selector)?;
        if self
            .records()?
            .iter()
            .any(|other| other.cloud_id != r.cloud_id && other.local_id == id)
        {
            return Err(problem("That device ID is already assigned."));
        }
        let mut s = self.settings()?;
        s.ids.insert(r.cloud_id, id);
        self.save(&s)
    }
    pub fn import_names(&self, devices: &[DiscoveredDevice]) -> Result<()> {
        let mut s = self.settings()?;
        let mut discovered = devices.to_vec();
        discovered.sort_by(|a, b| a.id.cmp(&b.id));
        discovered.dedup_by(|a, b| a.id == b.id);
        for device in &mut discovered {
            if !s.ids.contains_key(&device.id) {
                let next = s
                    .ids
                    .values()
                    .copied()
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| problem("No more local device IDs are available."))?;
                s.ids.insert(device.id.clone(), next);
            }
            device.name = device
                .name
                .chars()
                .filter(|c| !c.is_control())
                .take(60)
                .collect::<String>()
                .trim()
                .to_owned();
            device.mac = device
                .mac
                .chars()
                .filter(|c| c.is_ascii_hexdigit() || *c == ':')
                .collect();
            if device.name.is_empty() {
                device.name = format!("Air conditioner {}", s.ids[&device.id]);
            }
            if s.labels
                .get(&device.id)
                .is_none_or(|v| v.starts_with("AC · "))
            {
                s.labels.insert(device.id.clone(), device.name.clone());
            }
        }
        s.devices = discovered;
        self.save(&s)
    }
}
fn private_file(path: &Path, truncate: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .read(true)
        .create(true)
        .truncate(truncate);
    #[cfg(unix)]
    options.mode(0o600);
    options
        .open(path)
        .map_err(|_| problem("Cannot access controller data files."))
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|_| problem("Cannot read controller settings."))?;
    serde_json::from_slice(&bytes).map_err(|_| problem("Controller settings contain invalid JSON."))
}
pub(crate) fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", random_hex(8)));
    let result = (|| {
        let mut file = private_file(&temp, true)?;
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|_| problem("Cannot serialize controller data."))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| problem("Cannot save controller data."))?;
        fs::rename(&temp, path).map_err(|_| problem("Cannot replace controller data."))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

// Keep existing storage and credential identifiers so renaming preserves sessions and labels.
pub fn default_data_directory() -> PathBuf {
    if let Some(path) =
        std::env::var_os("AC_MAN_DATA_DIR").or_else(|| std::env::var_os("AC_CONTROL_DATA_DIR"))
    {
        return path.into();
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Library/Application Support/Midea Control");
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(path).join("ac-control");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/ac-control");
        }
    }
    PathBuf::from("data")
}

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct SavedAuth {
    pub account: String,
    pub password: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub session: Option<CloudSession>,
}
const SERVICE: &str = "local.midea-control.smarthome";
const ACCOUNT: &str = "SmartHome";
fn load_login_sync() -> Result<Option<SavedAuth>> {
    #[cfg(target_os = "macos")]
    {
        match security_framework::passwords::get_generic_password(SERVICE, ACCOUNT) {
            Ok(bytes) => serde_json::from_slice(&Zeroizing::new(bytes))
                .map(Some)
                .map_err(|_| problem("The saved Keychain login is invalid. Sign in again.")),
            Err(e) if e.code() == -25300 => Ok(None),
            Err(_) => Err(problem(
                "macOS could not read the saved login. Unlock or allow access to Keychain.",
            )),
        }
    }
    #[cfg(target_os = "linux")]
    {
        match linux_entry()?.get_secret() {
            Ok(bytes) => serde_json::from_slice(&Zeroizing::new(bytes))
                .map(Some)
                .map_err(|_| problem("The saved login is invalid. Sign in again.")),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(problem(
                "Cannot access Secret Service. Start and unlock your user-session keyring.",
            )),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(problem(
            "Saved login requires macOS Keychain or Linux Secret Service.",
        ))
    }
}
fn save_login_sync(auth: &SavedAuth) -> Result<()> {
    let bytes = Zeroizing::new(
        serde_json::to_vec(auth).map_err(|_| problem("Could not encode the saved login."))?,
    );
    #[cfg(target_os = "macos")]
    {
        security_framework::passwords::set_generic_password(SERVICE, ACCOUNT, &bytes)
            .map_err(|_| problem("macOS could not save the login in Keychain."))
    }
    #[cfg(target_os = "linux")]
    {
        linux_entry()?.set_secret(&bytes).map_err(|_| {
            problem("Cannot save login. Start and unlock your Secret Service keyring.")
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = bytes;
        Err(problem(
            "Saved login requires macOS Keychain or Linux Secret Service.",
        ))
    }
}
fn forget_login_sync() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        match security_framework::passwords::delete_generic_password(SERVICE, ACCOUNT) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(_) => Err(problem("macOS could not remove the saved Keychain login.")),
        }
    }
    #[cfg(target_os = "linux")]
    {
        match linux_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(problem("Cannot remove login from Secret Service.")),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(problem(
            "Saved login requires macOS Keychain or Linux Secret Service.",
        ))
    }
}

pub(crate) async fn load_login() -> Result<Option<SavedAuth>> {
    tokio::task::spawn_blocking(load_login_sync)
        .await
        .map_err(|_| problem("Credential store task failed."))?
}
pub(crate) async fn save_login(auth: &SavedAuth) -> Result<()> {
    let auth = auth.clone();
    tokio::task::spawn_blocking(move || save_login_sync(&auth))
        .await
        .map_err(|_| problem("Credential store task failed."))?
}
pub(crate) async fn forget_login() -> Result<()> {
    tokio::task::spawn_blocking(forget_login_sync)
        .await
        .map_err(|_| problem("Credential store task failed."))?
}

#[cfg(target_os = "linux")]
fn linux_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|_| problem("Cannot initialize the Secret Service keyring."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_ids_and_legacy_migration() {
        let dir = tempfile::tempdir().unwrap();
        atomic_json(
            &dir.path().join("settings.json"),
            &serde_json::json!({"labels":{"10001":"Bedroom"}}),
        )
        .unwrap();
        let store = Store::open(dir.path().into()).unwrap();
        assert!(store.records().unwrap().is_empty());
        store
            .import_names(&[
                DiscoveredDevice {
                    id: "10001".into(),
                    name: "Bedroom".into(),
                    mac: String::new(),
                },
                DiscoveredDevice {
                    id: "10002".into(),
                    name: "Kitchen".into(),
                    mac: String::new(),
                },
            ])
            .unwrap();
        assert_eq!(store.resolve("1").unwrap().label, "Bedroom");
        store.assign_id("1", 7).unwrap();
        store.rename("7", "Office").unwrap();
        assert_eq!(store.resolve("office").unwrap().local_id, 7);
        assert!(store.assign_id("7", 2).is_err());
        let again = Store::open(dir.path().into()).unwrap();
        assert_eq!(again.client_id, store.client_id);
        assert_eq!(again.resolve("7").unwrap().label, "Office");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dir.path().join("settings.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        for entry in fs::read_dir(dir.path()).unwrap().flatten() {
            let s = fs::read_to_string(entry.path()).unwrap();
            assert!(!s.contains("password"));
            assert!(!s.contains("access_token"));
        }
    }
}
