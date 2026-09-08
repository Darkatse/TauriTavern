use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use local_ip_address::list_afinet_netifas;
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tt_contracts::lan_discovery::{LanDiscoveredDevice, LanDiscoveryAnnouncement};
use tt_domain::errors::DomainError;
use tt_domain::models::lan_sync::validate_device_name;
use tt_ports::lan_discovery::{LanDeviceDiscovery, LanDiscoveryHost};
use ttsync_contract::peer::DeviceId;

const DISCOVERY_PORT: u16 = 53318;
const MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 168);
const DISCOVERY_PROTOCOL: &str = "tauritavern-lan-v2";
const MAX_PACKET_BYTES: usize = 2048;
const MAX_DEVICES: usize = 128;
const MAX_ADDRESSES_PER_DEVICE: usize = 8;
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const ADDRESS_TTL: Duration = Duration::from_secs(90);
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
    sockets: Vec<Arc<UdpSocket>>,
    receivers: Vec<JoinHandle<()>>,
    refresher: Option<JoinHandle<()>>,
}

impl DiscoveryRuntime {
    async fn stop_receivers(&mut self) {
        for receiver in &self.receivers {
            receiver.abort();
        }
        for receiver in self.receivers.drain(..) {
            let _ = receiver.await;
        }
        self.sockets.clear();
    }
}

impl Drop for DiscoveryRuntime {
    fn drop(&mut self) {
        for receiver in &self.receivers {
            receiver.abort();
        }
        if let Some(refresher) = &self.refresher {
            refresher.abort();
        }
    }
}

#[derive(Serialize, Deserialize)]
struct DiscoveryMessage {
    protocol: String,
    request: bool,
    device: Option<LanDiscoveryAnnouncement>,
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
        let mut peers = self.inner.peers.lock().expect("LAN discovery peer lock");
        let changed = peers.expire(Instant::now());
        let devices = peers.snapshot();
        if changed {
            self.inner.host.publish_changed(devices.clone());
        }
        devices
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
        if let Some(mut running) = runtime.take() {
            if let Some(refresher) = running.refresher.take() {
                refresher.abort();
                let _ = refresher.await;
            }
            running.stop_receivers().await;
        }
        *self
            .inner
            .local_device
            .write()
            .expect("LAN discovery local device lock") = None;
        let had_peers = {
            let mut peers = self.inner.peers.lock().expect("LAN discovery peer lock");
            let had_peers = !peers.devices.is_empty();
            peers.devices.clear();
            had_peers
        };
        if had_peers {
            self.inner.host.publish_changed(Vec::new());
        }
        self.inner.host.set_enabled(false).await
    }

    /// Re-enumerate interfaces so foreground resume and Wi-Fi changes replace stale sockets.
    pub async fn refresh(&self) -> Result<(), DomainError> {
        let mut runtime = self.inner.runtime.lock().await;
        // Android can release its multicast lock while Rust sockets survive suspension.
        self.inner.host.set_enabled(true).await?;
        let running = runtime.get_or_insert_with(|| DiscoveryRuntime {
            sockets: Vec::new(),
            receivers: Vec::new(),
            // Keep retrying when discovery was enabled before Wi-Fi became available.
            refresher: Some(spawn_refresh(Arc::downgrade(&self.inner))),
        });
        let sockets = match bind_sockets() {
            Ok(sockets) => sockets,
            Err(error) => {
                if running.sockets.is_empty() {
                    self.inner
                        .host
                        .set_enabled(false)
                        .await
                        .map_err(|cleanup| {
                            DomainError::InternalError(format!(
                                "{error}; discovery cleanup failed: {cleanup}"
                            ))
                        })?;
                }
                return Err(error);
            }
        };
        running.stop_receivers().await;
        running.sockets = sockets;
        running.receivers = receive_tasks(&running.sockets, &self.inner);

        self.snapshot();
        let message = DiscoveryMessage {
            protocol: DISCOVERY_PROTOCOL.to_string(),
            request: true,
            device: self
                .inner
                .local_device
                .read()
                .expect("LAN discovery local device lock")
                .clone(),
        };
        let payload = serde_json::to_vec(&message)
            .map_err(|error| DomainError::InvalidData(error.to_string()))?;
        send_multicast(&running.sockets, &payload).await
    }
}

#[async_trait]
impl LanDeviceDiscovery for LanPeerDiscovery {
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
        Ok(self.snapshot())
    }
}

fn spawn_refresh(inner: Weak<DiscoveryInner>) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(REFRESH_INTERVAL).await;
            let Some(inner) = inner.upgrade() else { return };
            if let Err(error) = (LanPeerDiscovery { inner }).refresh().await {
                tracing::warn!("LAN discovery refresh failed: {error}");
            }
        }
    })
}

fn receive_tasks(sockets: &[Arc<UdpSocket>], inner: &Arc<DiscoveryInner>) -> Vec<JoinHandle<()>> {
    sockets
        .iter()
        .map(|socket| tokio::spawn(receive_loop(socket.clone(), Arc::downgrade(inner))))
        .collect()
}

async fn receive_loop(socket: Arc<UdpSocket>, inner: Weak<DiscoveryInner>) {
    // Windows reports an oversized receive as a socket error instead of truncating it.
    // Read one full datagram, then enforce the smaller discovery message limit.
    let mut buffer = vec![0; 65_536];
    loop {
        let (size, source) = match socket.recv_from(&mut buffer).await {
            Ok(received) => received,
            Err(error) => {
                tracing::warn!("LAN discovery receive failed: {error}");
                return;
            }
        };
        let Some(inner) = inner.upgrade() else { return };
        let Some(response) = process_message(&inner, &buffer[..size], source.ip(), Instant::now())
        else {
            continue;
        };
        let payload = serde_json::to_vec(&response).expect("discovery message serializes");
        if let Err(error) = socket.send_to(&payload, source).await {
            tracing::warn!("LAN discovery reply to {source} failed: {error}");
        }
    }
}

fn process_message(
    inner: &DiscoveryInner,
    bytes: &[u8],
    source: IpAddr,
    now: Instant,
) -> Option<DiscoveryMessage> {
    let IpAddr::V4(source) = source else {
        return None;
    };
    if bytes.len() > MAX_PACKET_BYTES
        || source.is_unspecified()
        || source.is_multicast()
        || source.is_broadcast()
    {
        return None;
    }
    let message: DiscoveryMessage = serde_json::from_slice(bytes).ok()?;
    if message.protocol != DISCOVERY_PROTOCOL {
        return None;
    }
    let local = inner
        .local_device
        .read()
        .expect("LAN discovery local device lock")
        .clone();
    if let Some(device) = message.device {
        validate_announcement(&device).ok()?;
        if local
            .as_ref()
            .is_some_and(|local| local.device_id == device.device_id)
        {
            return None;
        }
        let mut peers = inner.peers.lock().expect("LAN discovery peer lock");
        if peers.record(device, source, now) {
            // Serialize snapshots with mutations so concurrent interfaces cannot publish
            // an older directory after a newer one.
            inner.host.publish_changed(peers.snapshot());
        }
    }
    if message.request {
        local.map(|device| DiscoveryMessage {
            protocol: DISCOVERY_PROTOCOL.to_string(),
            request: false,
            device: Some(device),
        })
    } else {
        None
    }
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

fn bind_sockets() -> Result<Vec<Arc<UdpSocket>>, DomainError> {
    let interfaces = list_afinet_netifas().map_err(|error| {
        DomainError::InternalError(format!("Cannot list LAN discovery interfaces: {error}"))
    })?;
    let mut addresses = interfaces
        .into_iter()
        .filter_map(|(_, ip)| match ip {
            IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast() => {
                Some(ip)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    let mut sockets = Vec::new();
    let mut last_error = None;
    for address in addresses {
        match bind_socket(address) {
            Ok(socket) => sockets.push(Arc::new(socket)),
            Err(error) => {
                tracing::warn!("Cannot join LAN discovery multicast on {address}: {error}");
                last_error = Some(format!(
                    "Cannot join LAN discovery multicast on {address}: {error}"
                ));
            }
        }
    }
    if sockets.is_empty() {
        return Err(DomainError::InternalError(last_error.unwrap_or_else(
            || "No IPv4 interface available for LAN discovery".to_string(),
        )));
    }
    Ok(sockets)
}

fn bind_socket(interface: Ipv4Addr) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    // Wildcard binding receives multicast on macOS; outgoing interface selection avoids
    // announcing every address through the default route on multihomed devices.
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).into())?;
    socket.join_multicast_v4(&MULTICAST_GROUP, &interface)?;
    socket.set_multicast_if_v4(&interface)?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_multicast_ttl_v4(1)?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

async fn send_multicast(sockets: &[Arc<UdpSocket>], payload: &[u8]) -> Result<(), DomainError> {
    let mut sent = false;
    for socket in sockets {
        match socket
            .send_to(payload, (MULTICAST_GROUP, DISCOVERY_PORT))
            .await
        {
            Ok(_) => sent = true,
            Err(error) => tracing::warn!("LAN discovery announcement failed: {error}"),
        }
    }
    if !sent {
        return Err(DomainError::InternalError(
            "LAN discovery announcement failed on every interface".to_string(),
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
    addresses: HashMap<String, Instant>,
}

impl PeerDirectory {
    fn record(&mut self, device: LanDiscoveryAnnouncement, source: Ipv4Addr, now: Instant) -> bool {
        let mut changed = self.expire(now);
        if self.devices.len() == MAX_DEVICES && !self.devices.contains_key(&device.device_id) {
            return changed;
        }
        let peer = self.devices.entry(device.device_id).or_insert_with(|| {
            changed = true;
            SeenDevice {
                device_name: device.device_name.clone(),
                platform: device.platform.clone(),
                spki_sha256: device.spki_sha256.clone(),
                addresses: HashMap::new(),
            }
        });
        changed |= peer.device_name != device.device_name
            || peer.spki_sha256 != device.spki_sha256
            || peer.platform != device.platform;
        peer.device_name = device.device_name;
        peer.platform = device.platform;
        peer.spki_sha256 = device.spki_sha256;
        let address = format!("https://{source}:{}", device.port);
        if !peer.addresses.contains_key(&address)
            && peer.addresses.len() == MAX_ADDRESSES_PER_DEVICE
        {
            let oldest = peer
                .addresses
                .iter()
                .min_by_key(|(_, seen)| **seen)
                .map(|(address, _)| address.clone())
                .expect("nonempty address cache");
            peer.addresses.remove(&oldest);
        }
        changed |= peer.addresses.insert(address, now).is_none();
        changed
    }

    fn expire(&mut self, now: Instant) -> bool {
        let mut changed = false;
        self.devices.retain(|_, peer| {
            peer.addresses.retain(|_, seen| {
                let fresh = now.duration_since(*seen) < ADDRESS_TTL;
                changed |= !fresh;
                fresh
            });
            !peer.addresses.is_empty()
        });
        changed
    }

    fn snapshot(&self) -> Vec<LanDiscoveredDevice> {
        let mut devices = self
            .devices
            .iter()
            .map(|(device_id, peer)| {
                let mut base_urls = peer.addresses.keys().cloned().collect::<Vec<_>>();
                base_urls.sort_unstable();
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
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingHost {
        snapshots: Mutex<Vec<Vec<LanDiscoveredDevice>>>,
    }

    #[async_trait]
    impl LanDiscoveryHost for RecordingHost {
        async fn set_enabled(&self, _enabled: bool) -> Result<(), DomainError> {
            Ok(())
        }

        fn publish_changed(&self, devices: Vec<LanDiscoveredDevice>) {
            self.snapshots.lock().unwrap().push(devices);
        }
    }

    #[test]
    fn discovery_ignores_bad_datagrams_and_merges_source_addresses_until_they_expire() {
        let host = Arc::new(RecordingHost::default());
        let discovery = LanPeerDiscovery::new(host.clone());
        let device_id = DeviceId::new("11111111-1111-4111-8111-111111111111".to_string()).unwrap();
        let packet = serde_json::to_vec(&DiscoveryMessage {
            protocol: DISCOVERY_PROTOCOL.to_string(),
            request: false,
            device: Some(LanDiscoveryAnnouncement {
                device_id: device_id.clone(),
                device_name: "Peer".to_string(),
                platform: Some("android".to_string()),
                port: 56000,
                spki_sha256: "a".repeat(43),
            }),
        })
        .unwrap();
        let now = Instant::now();
        let first = "192.168.1.10".parse().unwrap();
        let second = "192.168.1.20".parse().unwrap();
        process_message(&discovery.inner, b"not JSON", first, now);
        let mut oversized = packet.clone();
        oversized.resize(MAX_PACKET_BYTES + 1, b' ');
        process_message(&discovery.inner, &oversized, first, now);
        assert!(discovery.snapshot().is_empty());

        process_message(&discovery.inner, &packet, first, now);
        process_message(&discovery.inner, &packet, first, now);
        assert_eq!(host.snapshots.lock().unwrap().len(), 1);
        process_message(
            &discovery.inner,
            &packet,
            second,
            now + Duration::from_secs(30),
        );
        assert_eq!(
            discovery.candidate_urls(&device_id),
            ["https://192.168.1.10:56000", "https://192.168.1.20:56000"]
        );

        let mut renamed: DiscoveryMessage = serde_json::from_slice(&packet).unwrap();
        let device = renamed.device.as_mut().unwrap();
        device.device_name = "新设备名".to_string();
        device.platform = Some("ios".to_string());
        process_message(
            &discovery.inner,
            &serde_json::to_vec(&renamed).unwrap(),
            second,
            now + Duration::from_secs(30),
        );
        let snapshots = host.snapshots.lock().unwrap();
        let last = snapshots.last().unwrap();
        assert_eq!(last[0].device_name, "新设备名");
        assert_eq!(last[0].platform.as_deref(), Some("ios"));
        drop(snapshots);

        let mut peers = discovery.inner.peers.lock().unwrap();
        assert!(peers.expire(now + ADDRESS_TTL));
        assert_eq!(
            peers.snapshot()[0].base_urls,
            ["https://192.168.1.20:56000"]
        );
        assert!(peers.expire(now + ADDRESS_TTL + Duration::from_secs(30)));
        assert!(peers.snapshot().is_empty());
    }
}
