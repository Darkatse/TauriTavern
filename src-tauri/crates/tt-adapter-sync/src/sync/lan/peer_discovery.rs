use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tt_contracts::lan_discovery::{LanDiscoveredDevice, LanDiscoveryAnnouncement};
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::validate_device_name;
use tt_ports::lan_discovery::{LanDeviceDiscovery, LanDiscoveryHost};
use ttsync_contract::peer::DeviceId;

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "peer_discovery/apple.rs"]
mod backend;
#[cfg(any(target_os = "windows", target_os = "linux", target_os = "android"))]
#[path = "peer_discovery/mdns.rs"]
mod backend;

const SERVICE_TYPE: &str = "_tauritavern._tcp";
const MAX_DEVICES: usize = 128;
const MAX_ADDRESSES_PER_DEVICE: usize = 8;
const DISCOVERY_WINDOW: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct LanPeerDiscovery {
    inner: Arc<DiscoveryInner>,
}

struct DiscoveryInner {
    local_device: RwLock<Option<LanDiscoveryAnnouncement>>,
    peers: Mutex<PeerDirectory>,
    runtime: AsyncMutex<Option<DiscoveryRuntime>>,
    host: Arc<dyn LanDiscoveryHost>,
}

struct DiscoveryRuntime {
    backend: backend::Backend,
    receiver: JoinHandle<Result<(), DomainError>>,
}

impl DiscoveryRuntime {
    async fn stop(mut self) -> Result<(), DomainError> {
        self.receiver.abort();
        let received = (&mut self.receiver).await;
        let stopped = self.backend.stop().await;
        match received {
            Ok(result) => result.and(stopped),
            Err(error) if error.is_cancelled() => stopped,
            Err(error) => Err(discovery_error(error)),
        }
    }
}

impl Drop for DiscoveryRuntime {
    fn drop(&mut self) {
        self.receiver.abort();
    }
}

impl LanPeerDiscovery {
    pub fn new(host: Arc<dyn LanDiscoveryHost>) -> Self {
        Self {
            inner: Arc::new(DiscoveryInner {
                local_device: RwLock::new(None),
                peers: Mutex::new(PeerDirectory::default()),
                runtime: AsyncMutex::new(None),
                host,
            }),
        }
    }

    /// Announcing requires a running HTTPS listener; browsing does not.
    pub async fn set_local_device(
        &self,
        device: LanDiscoveryAnnouncement,
    ) -> Result<(), DomainError> {
        validate_announcement(&device)?;
        *self
            .inner
            .local_device
            .write()
            .expect("LAN discovery local device lock") = Some(device);
        self.refresh().await
    }

    pub fn snapshot(&self) -> Vec<LanDiscoveredDevice> {
        self.inner
            .peers
            .lock()
            .expect("LAN discovery peer lock")
            .snapshot()
    }

    pub fn candidate_urls(&self, device_id: &DeviceId) -> Vec<String> {
        self.snapshot()
            .into_iter()
            .find(|device| &device.device_id == device_id)
            .map(|device| device.base_urls)
            .unwrap_or_default()
    }

    pub async fn stop(&self) -> Result<(), DomainError> {
        let mut runtime = self.inner.runtime.lock().await;
        let stopped = match runtime.take() {
            Some(running) => running.stop().await,
            None => Ok(()),
        };
        *self
            .inner
            .local_device
            .write()
            .expect("LAN discovery local device lock") = None;
        self.inner.clear_peers();
        let released = self.inner.host.set_enabled(false).await;
        stopped.and(released)
    }

    pub async fn refresh(&self) -> Result<(), DomainError> {
        self.refresh_runtime(false).await
    }

    async fn refresh_runtime(&self, only_if_started: bool) -> Result<(), DomainError> {
        let mut runtime = self.inner.runtime.lock().await;
        if only_if_started && runtime.is_none() {
            return Ok(());
        }
        if runtime
            .as_ref()
            .is_some_and(|running| running.receiver.is_finished())
        {
            if let Err(error) = runtime
                .take()
                .expect("finished discovery runtime")
                .stop()
                .await
            {
                tracing::warn!("Restarting LAN discovery after failure: {error}");
            }
            self.inner.clear_peers();
        }
        if runtime.is_none() {
            let (backend, receiver) = backend::start(Arc::downgrade(&self.inner))?;
            *runtime = Some(DiscoveryRuntime { backend, receiver });
        }
        self.inner.host.set_enabled(true).await?;
        let local = self
            .inner
            .local_device
            .read()
            .expect("LAN discovery local device lock")
            .clone();
        if let Some(device) = local {
            runtime
                .as_mut()
                .expect("started discovery runtime")
                .backend
                .announce(&device)
                .await?;
        }
        Ok(())
    }
}

#[async_trait]
impl LanDeviceDiscovery for LanPeerDiscovery {
    async fn refresh_if_started(&self) -> Result<(), DomainError> {
        self.refresh_runtime(true).await
    }

    async fn set_device_name(&self, name: &str) -> Result<(), DomainError> {
        validate_device_name(name)?;
        let announcing = {
            let mut local = self
                .inner
                .local_device
                .write()
                .expect("LAN discovery local device lock");
            if let Some(device) = local.as_mut() {
                device.device_name = name.to_string();
            }
            local.is_some()
        };
        if announcing {
            self.refresh().await?;
        }
        Ok(())
    }

    async fn discover_devices(&self) -> Result<Vec<LanDiscoveredDevice>, DomainError> {
        self.refresh().await?;
        tokio::time::sleep(DISCOVERY_WINDOW).await;
        let mut runtime = self.inner.runtime.lock().await;
        if runtime
            .as_ref()
            .is_some_and(|running| running.receiver.is_finished())
        {
            let result = runtime
                .take()
                .expect("finished discovery runtime")
                .stop()
                .await;
            self.inner.clear_peers();
            let released = self.inner.host.set_enabled(false).await;
            result.and(released)?;
            return Err(discovery_error(
                "Device discovery stopped; refresh to try again",
            ));
        }
        Ok(self.snapshot())
    }
}

impl DiscoveryInner {
    fn clear_peers(&self) {
        let mut peers = self.peers.lock().expect("LAN discovery peer lock");
        if !peers.devices.is_empty() {
            peers.devices.clear();
            self.host.publish_changed(Vec::new());
        }
    }
}

fn update_peer(
    inner: &Weak<DiscoveryInner>,
    source: &str,
    device: Option<(LanDiscoveryAnnouncement, Vec<Ipv4Addr>)>,
) {
    let Some(inner) = inner.upgrade() else { return };
    let local_id = inner
        .local_device
        .read()
        .expect("LAN discovery local device lock")
        .as_ref()
        .map(|device| device.device_id.clone());
    let mut peers = inner.peers.lock().expect("LAN discovery peer lock");
    let previous = peers.snapshot();
    peers.remove_source(source);
    if let Some((device, addresses)) = device
        && local_id.as_ref() != Some(&device.device_id)
    {
        peers.record(source, device, addresses);
    }
    let devices = peers.snapshot();
    if devices != previous {
        // Serialize publication with mutations, including callbacks from different interfaces.
        inner.host.publish_changed(devices);
    }
}

fn discovery_error(error: impl std::fmt::Display) -> DomainError {
    DomainError::Transient(format!("LAN device discovery failed: {error}"))
}

fn txt_properties(device: &LanDiscoveryAnnouncement) -> Vec<(&'static str, Vec<u8>)> {
    // Two fixed halves preserve the existing 64-character name limit within TXT's 255-byte entries.
    let mut name = device.device_name.chars();
    let first: String = name.by_ref().take(32).collect();
    let second: String = name.collect();
    vec![
        ("v", b"2".to_vec()),
        ("id", device.device_id.as_str().as_bytes().to_vec()),
        ("name", first.into_bytes()),
        ("name2", second.into_bytes()),
        (
            "platform",
            device
                .platform
                .as_deref()
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
        ),
        ("spki", device.spki_sha256.as_bytes().to_vec()),
    ]
}

fn decode_device<'a>(
    port: u16,
    property: impl Fn(&str) -> Option<&'a [u8]>,
) -> Result<LanDiscoveryAnnouncement, DomainError> {
    let text = |key: &str| {
        std::str::from_utf8(property(key).unwrap_or_default()).map_err(|error| {
            DomainError::InvalidData(format!("Invalid LAN discovery {key}: {error}"))
        })
    };
    if text("v")? != "2" {
        return Err(DomainError::InvalidData(
            "Unsupported LAN discovery version".to_string(),
        ));
    }
    let platform = text("platform")?;
    let device = LanDiscoveryAnnouncement {
        device_id: DeviceId::new(text("id")?.to_string())
            .map_err(|error| DomainError::InvalidData(error.to_string()))?,
        device_name: format!("{}{}", text("name")?, text("name2")?),
        platform: (!platform.is_empty()).then(|| platform.to_string()),
        port,
        spki_sha256: text("spki")?.to_string(),
    };
    validate_announcement(&device)?;
    Ok(device)
}

pub(crate) fn validate_announcement(device: &LanDiscoveryAnnouncement) -> Result<(), DomainError> {
    validate_device_name(&device.device_name)?;
    if device.port == 0
        || device
            .platform
            .as_ref()
            .is_some_and(|value| value.len() > 32 || !value.is_ascii())
        || device.spki_sha256.len() != 43
        || !device
            .spki_sha256
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(DomainError::InvalidData(
            "Invalid LAN discovery announcement".to_string(),
        ));
    }
    Ok(())
}

#[derive(Default)]
struct PeerDirectory {
    devices: HashMap<DeviceId, SeenDevice>,
}

struct SeenDevice {
    device_name: String,
    platform: Option<String>,
    spki_sha256: String,
    sources: HashMap<String, Vec<String>>,
}

impl PeerDirectory {
    fn record(&mut self, source: &str, device: LanDiscoveryAnnouncement, addresses: Vec<Ipv4Addr>) {
        let mut base_urls = addresses
            .into_iter()
            .filter(|ip| !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast())
            .map(|ip| format!("https://{ip}:{}", device.port))
            .collect::<Vec<_>>();
        base_urls.sort_unstable();
        base_urls.dedup();
        base_urls.truncate(MAX_ADDRESSES_PER_DEVICE);
        if base_urls.is_empty()
            || (self.devices.len() == MAX_DEVICES && !self.devices.contains_key(&device.device_id))
        {
            return;
        }
        let peer = self
            .devices
            .entry(device.device_id)
            .or_insert_with(|| SeenDevice {
                device_name: device.device_name.clone(),
                platform: device.platform.clone(),
                spki_sha256: device.spki_sha256.clone(),
                sources: HashMap::new(),
            });
        peer.device_name = device.device_name;
        peer.platform = device.platform;
        peer.spki_sha256 = device.spki_sha256;
        peer.sources.insert(source.to_string(), base_urls);
    }

    fn remove_source(&mut self, source: &str) {
        // ponytail: scan at most 128 peers instead of maintaining a second source-to-device index.
        self.devices.retain(|_, peer| {
            peer.sources.remove(source);
            !peer.sources.is_empty()
        });
    }

    fn snapshot(&self) -> Vec<LanDiscoveredDevice> {
        let mut devices = self
            .devices
            .iter()
            .map(|(device_id, peer)| {
                let mut base_urls = peer.sources.values().flatten().cloned().collect::<Vec<_>>();
                base_urls.sort_unstable();
                base_urls.dedup();
                base_urls.truncate(MAX_ADDRESSES_PER_DEVICE);
                LanDiscoveredDevice {
                    device_id: device_id.clone(),
                    device_name: peer.device_name.clone(),
                    platform: peer.platform.clone(),
                    base_urls,
                    spki_sha256: peer.spki_sha256.clone(),
                }
            })
            .collect::<Vec<_>>();
        devices.sort_by(|a, b| a.device_id.as_str().cmp(b.device_id.as_str()));
        devices
    }
}

#[cfg(test)]
mod tests;
