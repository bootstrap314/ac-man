use crate::cloud::{CloudClient, DiscoveredDevice, session_expired};
use crate::protocol::{self, Capabilities, State};
use crate::storage::{self, Record, SavedAuth, Store};
use crate::{
    ANGLE_NAMES, AcError, AppSnapshot, DeviceSnapshot, FAN_NAMES, MODE_NAMES, Result, SWING_NAMES,
    SettingsChange, choices, problem,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use std::{future::Future, pin::Pin};
use tokio::{runtime::Runtime, sync::Mutex};
use zeroize::Zeroizing;

struct Inner {
    store: Store,
    cloud: CloudClient,
    auth: Option<SavedAuth>,
    remember: bool,
    owned: HashSet<String>,
    states: HashMap<String, State>,
    caps: HashMap<String, Capabilities>,
    errors: HashMap<String, String>,
    updated: HashMap<String, f64>,
    sequence: u8,
    session_revision: u64,
}
pub struct AcController {
    inner: Arc<Mutex<Inner>>,
}

#[boltffi::export]
impl AcController {
    pub fn new(data_directory: String) -> std::result::Result<Self, AcError> {
        let store = Store::open(PathBuf::from(data_directory))?;
        let cloud = CloudClient::new(store.client_id.clone())?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                store,
                cloud,
                auth: None,
                remember: false,
                owned: HashSet::new(),
                states: HashMap::new(),
                caps: HashMap::new(),
                errors: HashMap::new(),
                updated: HashMap::new(),
                sequence: 0,
                session_revision: 0,
            })),
        })
    }
    pub async fn snapshot(&self) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move { inner.lock().await.snapshot() }).await
    }
    pub async fn restore_login(&self) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
            let mut s = inner.lock().await;
            let _lock = s.store.lock_async().await?;
            s.restore().await?;
            s.finish()
        })
        .await
    }
    pub async fn login(
        &self,
        account: String,
        password: String,
        remember: bool,
    ) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
            let password = Zeroizing::new(password);
            let mut s = inner.lock().await;
            let _lock = s.store.lock_async().await?;
            let account = account.trim().to_owned();
            let mut cloud = CloudClient::new(s.store.client_id.clone())?;
            cloud.login(&account, &password).await?;
            let devices = cloud.appliances().await?;
            let owned = s.account_devices(&devices)?;
            let auth = SavedAuth {
                account,
                password: password.to_string(),
                client_id: s.store.client_id.clone(),
                session: cloud.session.clone(),
            };
            if remember {
                storage::save_login(&auth).await?;
            }
            s.store.import_names(&devices)?;
            s.cloud = cloud;
            s.auth = Some(auth);
            s.remember = remember;
            s.owned = owned;
            s.session_revision = s.session_revision.wrapping_add(1);
            s.states.clear();
            s.caps.clear();
            s.errors.clear();
            s.updated.clear();
            s.finish()
        })
        .await
    }
    pub async fn logout(&self) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
            let mut s = inner.lock().await;
            let _lock = s.store.lock_async().await?;
            storage::forget_login().await?;
            s.cloud.session = None;
            s.auth = None;
            s.remember = false;
            s.session_revision = s.session_revision.wrapping_add(1);
            s.owned.clear();
            s.states.clear();
            s.caps.clear();
            s.errors.clear();
            s.updated.clear();
            s.finish()
        })
        .await
    }
    pub async fn refresh(&self) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
            let (records, session_revision) = {
                let mut s = inner.lock().await;
                let _lock = s.store.lock_async().await?;
                s.ensure_login().await?;
                (s.store.records()?, s.session_revision)
            };
            refresh_devices(inner, records, session_revision, read_cloud_device).await
        })
        .await
    }
    pub async fn device_status(
        &self,
        selector: String,
    ) -> std::result::Result<DeviceSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
            let mut s = inner.lock().await;
            let _lock = s.store.lock_async().await?;
            let record = s.store.resolve(&selector)?;
            s.ensure_login().await?;
            match s.read_device(&record).await {
                Ok(_) => {
                    let snapshot = s.finish()?;
                    snapshot
                        .devices
                        .into_iter()
                        .find(|d| d.id == record.cloud_id)
                        .ok_or_else(|| problem("Unknown AC."))
                }
                Err(e) => {
                    s.errors.insert(record.cloud_id, e.to_string());
                    let _ = s.finish();
                    Err(e)
                }
            }
        })
        .await
    }
    pub async fn control(
        &self,
        selector: String,
        change: SettingsChange,
    ) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
        let mut s=inner.lock().await; let _lock=s.store.lock_async().await?;
        let record=s.store.resolve(&selector)?;
        s.ensure_login().await?;
        let mut state=s.read_device(&record).await?;
        let caps=s.caps.get(&record.cloud_id).ok_or_else(||problem("Read this AC's supported settings before changing it."))?;
        state.apply(&change,caps)?;
        let regular = change.power.is_some() || change.temperature.is_some() || change.mode.is_some()
            || change.fan.is_some() || change.swing.is_some() || change.boost.is_some() || change.eco.is_some();
        let frame=regular.then(|| state.encode(s.next_sequence()));
        let angles=protocol::angle_command(&change,s.next_sequence());
        // Never automatically retry a write: a lost acknowledgement may still mean it applied.
        let result=async {
            if let Some(frame) = frame { s.cloud.send_frame(&record.cloud_id,&frame).await?; }
            if let Some(frame) = angles { s.cloud.send_frame(&record.cloud_id,&frame).await?; }
            for attempt in 0..3 {
                tokio::time::sleep(Duration::from_millis(if attempt==0 {500} else {1000})).await;
                let actual=s.read_device(&record).await?;
                if actual.matches(&change) {return Ok(());}
            }
            Err(problem("The AC reported different settings. Refresh and check the unit before retrying."))
        }.await;
        if let Err(e)=result {
            s.states.remove(&record.cloud_id);
            s.errors.insert(record.cloud_id,"The change could not be confirmed. Refresh before trying again.".into());
            let _=s.finish(); return Err(e);
        }
        s.finish()

        }).await
    }
    pub async fn rename(
        &self,
        selector: String,
        label: String,
    ) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
            let s = inner.lock().await;
            let _lock = s.store.lock_async().await?;
            s.store.rename(&selector, &label)?;
            s.finish()
        })
        .await
    }
    pub async fn assign_id(
        &self,
        selector: String,
        id: u32,
    ) -> std::result::Result<AppSnapshot, AcError> {
        let inner = self.inner.clone();
        dispatch(async move {
            let s = inner.lock().await;
            let _lock = s.store.lock_async().await?;
            s.store.assign_id(&selector, id)?;
            s.finish()
        })
        .await
    }
}
// Keep reads serialized with commands, but allow queued commands between ACs.
// The injected reader keeps the scheduling behavior testable without a cloud account.
type DeviceRead<'a> = Pin<Box<dyn Future<Output = Result<State>> + Send + 'a>>;
fn read_cloud_device<'a>(s: &'a mut Inner, record: &'a Record) -> DeviceRead<'a> {
    Box::pin(s.read_device(record))
}
async fn refresh_devices(
    inner: Arc<Mutex<Inner>>,
    records: Vec<Record>,
    session_revision: u64,
    mut read: impl for<'a> FnMut(&'a mut Inner, &'a Record) -> DeviceRead<'a> + Send,
) -> Result<AppSnapshot> {
    for record in records {
        {
            let mut s = inner.lock().await;
            // An explicit login/logout supersedes this refresh's inventory and session.
            if s.session_revision != session_revision || s.cloud.session.is_none() {
                break;
            }
            let _lock = s.store.lock_async().await?;
            if s.owned.contains(&record.cloud_id)
                && let Err(error) = read(&mut s, &record).await
            {
                s.errors.insert(record.cloud_id, error.to_string());
            }
        }
        // Both guards have dropped before yielding, including the cross-process lock.
        tokio::task::yield_now().await;
    }
    let s = inner.lock().await;
    let _lock = s.store.lock_async().await?;
    // Read the current shared state; an intervening command may have changed an AC.
    s.finish()
}
async fn dispatch<T: Send + 'static>(
    work: impl Future<Output = Result<T>> + Send + 'static,
) -> Result<T> {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Tokio runtime")
    });
    runtime
        .spawn(work)
        .await
        .map_err(|_| problem("The controller task stopped unexpectedly."))?
}

impl Inner {
    fn next_sequence(&mut self) -> u8 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }
    fn account_devices(&self, devices: &[DiscoveredDevice]) -> Result<HashSet<String>> {
        Ok(devices.iter().map(|d| d.id.clone()).collect())
    }
    async fn restore(&mut self) -> Result<()> {
        let Some(mut auth) = storage::load_login().await? else {
            return Ok(());
        };
        let mut cloud = CloudClient::new(self.store.client_id.clone())?;
        if auth.client_id == self.store.client_id
            && let Some(session) = auth.session.clone()
        {
            cloud.use_session(session)?;
        }
        if cloud.session.is_none() {
            cloud.login(&auth.account, &auth.password).await?;
        }
        let devices = cloud
            .appliances_with_relogin(&auth.account, &auth.password)
            .await?;
        let owned = self.account_devices(&devices)?;
        auth.client_id = self.store.client_id.clone();
        auth.session = cloud.session.clone();
        storage::save_login(&auth).await?;
        self.store.import_names(&devices)?;
        self.cloud = cloud;
        self.auth = Some(auth);
        self.remember = true;
        self.owned = owned;
        self.session_revision = self.session_revision.wrapping_add(1);
        Ok(())
    }
    async fn ensure_login(&mut self) -> Result<()> {
        if self.cloud.session.is_none() {
            self.restore().await?;
        }
        if self.cloud.session.is_none() {
            return Err(problem("Sign in first: ac-man login"));
        }
        Ok(())
    }
    async fn renew_login(&mut self) -> Result<()> {
        let auth = self
            .auth
            .as_mut()
            .ok_or_else(|| problem("Session expired. Sign in again."))?;
        self.cloud.login(&auth.account, &auth.password).await?;
        auth.session = self.cloud.session.clone();
        if self.remember {
            storage::save_login(auth).await?;
        }
        let devices = self.cloud.appliances().await?;
        self.owned = self.account_devices(&devices)?;
        self.store.import_names(&devices)?;
        Ok(())
    }
    async fn read_device(&mut self, record: &Record) -> Result<State> {
        match self.read_once(record).await {
            Err(ref e) if session_expired(e) => {
                self.renew_login().await?;
                self.read_once(record).await
            }
            result => result,
        }
    }
    async fn read_once(&mut self, record: &Record) -> Result<State> {
        if !self.owned.contains(&record.cloud_id) {
            return Err(problem(
                "This AC is not available in the signed-in account.",
            ));
        }
        if !self.caps.contains_key(&record.cloud_id) {
            let frame = protocol::capability_query(false, self.next_sequence());
            let response = self
                .cloud
                .send_frame(&record.cloud_id, &frame)
                .await?
                .ok_or_else(|| problem("The AC did not reply to its capabilities query."))?;
            let caps = Capabilities::decode(&response)?;
            self.caps.insert(record.cloud_id.clone(), caps);
        }
        // Keep first-page controls available, but don't permanently cache a
        // missing optional page. Try it once per read until a valid reply arrives.
        if self
            .caps
            .get(&record.cloud_id)
            .is_some_and(|caps| caps.additional)
        {
            let frame = protocol::capability_query(true, self.next_sequence());
            if let Ok(Some(response)) = self.cloud.send_frame(&record.cloud_id, &frame).await
                && let Ok(additional) = Capabilities::decode(&response)
                && let Some(caps) = self.caps.get_mut(&record.cloud_id)
            {
                caps.merge(additional);
            }
        }
        let frame = protocol::state_query(self.next_sequence());
        let response = self
            .cloud
            .send_frame(&record.cloud_id, &frame)
            .await?
            .ok_or_else(|| problem("The AC did not return its current settings."))?;
        let mut state = State::decode(&response)?;
        let sequence = self.next_sequence();
        let query = self
            .caps
            .get(&record.cloud_id)
            .and_then(|caps| protocol::angle_query(caps, sequence));
        // Optional properties must not make basic controls unavailable on firmware
        // that advertises a capability but fails to answer the property query.
        if let Some(query) = query
            && let Ok(Some(response)) = self.cloud.send_frame(&record.cloud_id, &query).await
            && let Ok((vertical, horizontal)) = protocol::decode_angles(&response)
        {
            state.vertical_angle = vertical;
            state.horizontal_angle = horizontal;
        }
        self.states.insert(record.cloud_id.clone(), state.clone());
        self.errors.remove(&record.cloud_id);
        self.updated.insert(
            record.cloud_id.clone(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64(),
        );
        Ok(state)
    }
    fn snapshot(&self) -> Result<AppSnapshot> {
        let mut devices = Vec::new();
        for r in self.store.records()? {
            let error = self.errors.get(&r.cloud_id).cloned().unwrap_or_else(|| {
                if self.cloud.session.is_some() && !self.owned.contains(&r.cloud_id) {
                    "Not available in this SmartHome account.".into()
                } else {
                    String::new()
                }
            });
            let state = self.states.get(&r.cloud_id).filter(|_| error.is_empty());
            let caps = self.caps.get(&r.cloud_id);
            devices.push(DeviceSnapshot {
                connected: self.owned.contains(&r.cloud_id),
                online: state.is_some(),
                error,
                updated: self.updated.get(&r.cloud_id).copied().unwrap_or(0.0),
                power: state.is_some_and(|s| s.power),
                temperature: state.map(|s| s.temperature),
                indoor: state.and_then(|s| s.indoor),
                outdoor: state.and_then(|s| s.outdoor),
                mode: state.map_or(0, |s| s.mode),
                fan: state.map_or(102, |s| s.fan),
                swing: state.map_or(0, |s| s.swing),
                // Zero disables the indexed-angle setting; it does not identify
                // the physical position of the louver. Keep raw state for command
                // confirmation, but expose no selected position to clients.
                vertical_angle: state.and_then(|s| s.vertical_angle).filter(|v| *v != 0),
                horizontal_angle: state.and_then(|s| s.horizontal_angle).filter(|v| *v != 0),
                vertical_angles: if caps.is_some_and(|c| c.vertical_angle) {
                    choices(&[0, 1, 25, 50, 75, 100], ANGLE_NAMES)
                } else {
                    vec![]
                },
                horizontal_angles: if caps.is_some_and(|c| c.horizontal_angle) {
                    choices(&[0, 1, 25, 50, 75, 100], ANGLE_NAMES)
                } else {
                    vec![]
                },
                boost: state.is_some_and(|s| s.boost),
                eco: state.is_some_and(|s| s.eco),
                custom_fan: caps.is_some_and(|c| c.custom_fan),
                supports_boost: caps.is_some_and(|c| c.boost),
                supports_eco: caps.is_some_and(|c| c.eco),
                min_temperature: caps.map_or(16.0, |c| c.min_temp),
                max_temperature: caps.map_or(30.0, |c| c.max_temp),
                modes: choices(caps.map_or(&[], |c| c.modes.as_slice()), MODE_NAMES),
                fans: choices(caps.map_or(&[], |c| c.fans.as_slice()), FAN_NAMES),
                swings: choices(caps.map_or(&[], |c| c.swings.as_slice()), SWING_NAMES),
                id: r.cloud_id,
                local_id: r.local_id,
                label: r.label,
                mac: r.mac,
            });
        }
        Ok(AppSnapshot {
            signed_in: self.cloud.session.is_some(),
            devices,
            route: self.cloud.route.clone(),
        })
    }
    fn finish(&self) -> Result<AppSnapshot> {
        let snapshot = self.snapshot()?;
        storage::atomic_json(
            &self.store.directory.join("status.json"),
            &serde_json::json!({"ok":true,"data":snapshot}),
        )?;
        Ok(snapshot)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::CloudSession;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        task::Poll,
    };
    use tokio::sync::oneshot;

    fn reference_state() -> State {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/protocol.json")).unwrap();
        State::decode(&hex::decode(fixture["state_response"].as_str().unwrap()).unwrap()).unwrap()
    }

    async fn refresh_test_controller() -> (tempfile::TempDir, AcController, Vec<Record>) {
        let dir = tempfile::tempdir().unwrap();
        let c = AcController::new(dir.path().to_string_lossy().into()).unwrap();
        let records = {
            let mut s = c.inner.lock().await;
            s.store
                .import_names(&[
                    DiscoveredDevice {
                        id: "10001".into(),
                        name: "Study".into(),
                        mac: String::new(),
                    },
                    DiscoveredDevice {
                        id: "10002".into(),
                        name: "Office".into(),
                        mac: String::new(),
                    },
                ])
                .unwrap();
            s.cloud.session = Some(CloudSession {
                base_url: "https://example.appsmb.com/".into(),
                uid: String::new(),
                access_token: String::new(),
                data_key: String::new(),
                data_iv: String::new(),
            });
            s.owned = HashSet::from(["10001".into(), "10002".into()]);
            s.store.records().unwrap()
        };
        (dir, c, records)
    }

    #[tokio::test]
    async fn snapshots_distinguish_reported_positions_from_zero_angle_settings() {
        let (_dir, c, _records) = refresh_test_controller().await;
        let mut inner = c.inner.lock().await;
        // Real property-response shapes; contain no device or account identifiers.
        for (reply, expected) in [
            ("aa18ac00000000000203b10209000001000a0000010000c8a7", None),
            (
                "aa18ac00000000000203b102090000014b0a0000010000b96b",
                Some(75),
            ),
        ] {
            let (vertical, horizontal) =
                protocol::decode_angles(&hex::decode(reply).unwrap()).unwrap();
            let mut state = reference_state();
            state.vertical_angle = vertical;
            state.horizontal_angle = horizontal;
            // Do not change the meaning of explicit zero-angle commands.
            assert!(state.matches(&SettingsChange {
                vertical_angle: vertical,
                horizontal_angle: horizontal,
                ..Default::default()
            }));
            inner.states.insert("10001".into(), state);
            let snapshot = inner.snapshot().unwrap();
            let device = snapshot.devices.iter().find(|d| d.id == "10001").unwrap();
            assert_eq!(device.vertical_angle, expected);
            assert_eq!(device.horizontal_angle, None);
        }
    }

    #[tokio::test]
    async fn refresh_yields_both_locks_and_preserves_an_intervening_command() {
        let (dir, c, records) = refresh_test_controller().await;
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let mut first = Some((started_tx, release_rx));
        let mut refresh = tokio::spawn(refresh_devices(
            c.inner.clone(),
            records,
            0,
            move |s, record| {
                let barrier = first.take();
                Box::pin(async move {
                    if let Some((started, release)) = barrier {
                        started.send(()).unwrap();
                        release.await.unwrap();
                    } else {
                        assert_eq!(
                            s.states["10001"].temperature, 24.0,
                            "queued command must run before the next read"
                        );
                    }
                    let state = reference_state();
                    s.states.insert(record.cloud_id.clone(), state.clone());
                    Ok(state)
                })
            },
        ));
        started_rx.await.unwrap();

        let update = async {
            let mut s = c.inner.lock().await;
            // This would deadlock if refresh released only the in-process mutex.
            let _file = s.store.lock_async().await.unwrap();
            s.states.get_mut("10001").unwrap().temperature = 24.0;
        };
        tokio::pin!(update);
        // Poll once while the first read owns the mutex, guaranteeing FIFO queueing.
        std::future::poll_fn(|cx| {
            assert!(update.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        release_tx.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let (snapshot, ()) = tokio::join!(&mut refresh, &mut update);
            snapshot.unwrap()
        })
        .await;
        if result.is_err() {
            refresh.abort();
        }
        let snapshot = result.unwrap().unwrap();
        assert_eq!(snapshot.devices[0].temperature, Some(24.0));
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("status.json")).unwrap())
                .unwrap();
        assert_eq!(saved["data"]["devices"][0]["temperature"], 24.0);
    }

    #[tokio::test]
    async fn changed_session_cancels_remaining_refresh_reads() {
        let (_dir, c, records) = refresh_test_controller().await;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let mut barrier = Some((started_tx, release_rx));
        let mut refresh = tokio::spawn(refresh_devices(
            c.inner.clone(),
            records,
            0,
            move |_s, _record| {
                let first = barrier.take();
                observed.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move {
                    if let Some((started, release)) = first {
                        started.send(()).unwrap();
                        release.await.unwrap();
                    }
                    Ok(reference_state())
                })
            },
        ));
        started_rx.await.unwrap();
        let change_session = async {
            let mut s = c.inner.lock().await;
            let _file = s.store.lock_async().await.unwrap();
            s.session_revision += 1;
            s.cloud.session = None;
            s.owned.clear();
        };
        tokio::pin!(change_session);
        std::future::poll_fn(|cx| {
            assert!(change_session.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        release_tx.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            let (snapshot, ()) = tokio::join!(&mut refresh, &mut change_session);
            snapshot.unwrap()
        })
        .await;
        if result.is_err() {
            refresh.abort();
        }
        assert!(!result.unwrap().unwrap().signed_in);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn snapshot_and_local_edits_need_no_credentials_or_network() {
        let dir = tempfile::tempdir().unwrap();
        let c = AcController::new(dir.path().to_string_lossy().into()).unwrap();
        assert!(!c.snapshot().await.unwrap().signed_in);
        assert!(c.snapshot().await.unwrap().devices.is_empty());
        c.inner
            .lock()
            .await
            .store
            .import_names(&[DiscoveredDevice {
                id: "10001".into(),
                name: "Study".into(),
                mac: String::new(),
            }])
            .unwrap();
        c.rename("1".into(), "Office".into()).await.unwrap();
        c.assign_id("Office".into(), 8).await.unwrap();
        let snapshot = c.snapshot().await.unwrap();
        assert_eq!(snapshot.devices[0].local_id, 8);
        assert_eq!(snapshot.devices[0].label, "Office");
        let json = serde_json::to_string(&snapshot).unwrap();
        for secret in ["password", "access_token", "data_key", "data_iv"] {
            assert!(!json.contains(secret));
        }
    }
}
