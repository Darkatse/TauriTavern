use super::*;

#[derive(Default)]
struct RecordingHost {
    snapshots: Mutex<Vec<Vec<LanDiscoveredDevice>>>,
    changed: tokio::sync::Notify,
}

#[async_trait]
impl LanDiscoveryHost for RecordingHost {
    async fn set_enabled(&self, _enabled: bool) -> Result<(), DomainError> {
        Ok(())
    }

    fn publish_changed(&self, devices: Vec<LanDiscoveredDevice>) {
        self.snapshots.lock().unwrap().push(devices);
        self.changed.notify_one();
    }
}

fn announcement(name: &str) -> LanDiscoveryAnnouncement {
    LanDiscoveryAnnouncement {
        device_id: DeviceId::new(uuid::Uuid::new_v4().to_string()).unwrap(),
        device_name: name.to_string(),
        platform: Some("android".to_string()),
        port: 56000,
        spki_sha256: "a".repeat(43),
    }
}

#[test]
fn discovery_preserves_unicode_names_and_remaining_interfaces_when_a_service_disappears() {
    let host = Arc::new(RecordingHost::default());
    let discovery = LanPeerDiscovery::new(host.clone());
    let inner = Arc::downgrade(&discovery.inner);
    let original = announcement(&"🦀".repeat(64));
    let mut txt = txt_properties(&original)
        .into_iter()
        .collect::<HashMap<_, _>>();
    let device = decode_device(original.port, |key| txt.get(key).map(Vec::as_slice)).unwrap();
    assert_eq!(device.device_name, original.device_name);
    txt.remove("spki");
    assert!(decode_device(original.port, |key| txt.get(key).map(Vec::as_slice)).is_err());

    let first = vec!["192.168.1.10".parse().unwrap()];
    let second = vec!["192.168.1.20".parse().unwrap()];
    update_peer(&inner, "wifi", Some((device.clone(), first.clone())));
    update_peer(&inner, "wifi", Some((device.clone(), first)));
    assert_eq!(host.snapshots.lock().unwrap().len(), 1);
    update_peer(&inner, "ethernet", Some((device.clone(), second.clone())));
    assert_eq!(
        discovery.candidate_urls(&device.device_id),
        ["https://192.168.1.10:56000", "https://192.168.1.20:56000",]
    );

    let mut renamed = device.clone();
    renamed.device_name = "新设备名".to_string();
    renamed.platform = Some("ios".to_string());
    update_peer(&inner, "ethernet", Some((renamed, second)));
    let snapshot = discovery.snapshot();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].device_name, "新设备名");
    assert_eq!(snapshot[0].platform.as_deref(), Some("ios"));
    update_peer(&inner, "wifi", None);
    assert_eq!(
        discovery.candidate_urls(&device.device_id),
        ["https://192.168.1.20:56000"]
    );
    update_peer(&inner, "ethernet", None);
    assert!(discovery.snapshot().is_empty());
}

#[cfg(target_os = "macos")]
#[path = "mdns.rs"]
mod mdns;

#[cfg(target_os = "macos")]
async fn wait_for_device(
    discovery: &LanPeerDiscovery,
    host: &RecordingHost,
    id: &DeviceId,
    name: Option<&str>,
) {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let changed = host.changed.notified();
            let devices = discovery.snapshot();
            let device = devices.iter().find(|device| &device.device_id == id);
            if match name {
                Some(name) => device.is_some_and(|device| device.device_name == name),
                None => device.is_none(),
            } {
                break;
            }
            changed.await;
        }
    })
    .await
    .expect("device discovery update");
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires access to local multicast networking"]
async fn system_bonjour_and_portable_mdns_discover_rename_and_remove_each_other() {
    let apple_host = Arc::new(RecordingHost::default());
    let portable_host = Arc::new(RecordingHost::default());
    let apple = LanPeerDiscovery::new(apple_host.clone());
    let portable = LanPeerDiscovery::new(portable_host.clone());
    let apple_device = announcement("Apple discovery test");
    let mut portable_device = announcement("Portable discovery test");
    let (mut backend, receiver) = mdns::start(Arc::downgrade(&portable.inner)).unwrap();

    apple.set_local_device(apple_device.clone()).await.unwrap();
    backend.announce(&portable_device).await.unwrap();
    wait_for_device(
        &apple,
        &apple_host,
        &portable_device.device_id,
        Some(&portable_device.device_name),
    )
    .await;
    wait_for_device(
        &portable,
        &portable_host,
        &apple_device.device_id,
        Some(&apple_device.device_name),
    )
    .await;

    portable_device.device_name = "🦀".repeat(64);
    backend.announce(&portable_device).await.unwrap();
    wait_for_device(
        &apple,
        &apple_host,
        &portable_device.device_id,
        Some(&portable_device.device_name),
    )
    .await;
    apple.set_device_name("已改名的 Apple 设备").await.unwrap();
    wait_for_device(
        &portable,
        &portable_host,
        &apple_device.device_id,
        Some("已改名的 Apple 设备"),
    )
    .await;
    apple.stop().await.unwrap();
    wait_for_device(&portable, &portable_host, &apple_device.device_id, None).await;

    apple.discover_devices().await.unwrap();
    wait_for_device(
        &apple,
        &apple_host,
        &portable_device.device_id,
        Some(&portable_device.device_name),
    )
    .await;
    receiver.abort();
    let _ = receiver.await;
    backend.stop().await.unwrap();
    wait_for_device(&apple, &apple_host, &portable_device.device_id, None).await;
    apple.stop().await.unwrap();
}
