use std::sync::Weak;

use mdns_sd::{DaemonEvent, IfKind, ServiceDaemon, ServiceEvent, ServiceInfo, TxtProperty};
use tokio::task::JoinHandle;
use tt_contracts::lan_discovery::LanDiscoveryAnnouncement;
use tt_domain::errors::DomainError;

use super::{
    DiscoveryInner, SERVICE_TYPE, decode_device, discovery_error, txt_properties, update_peer,
};

pub(super) struct Backend {
    daemon: ServiceDaemon,
}

pub(super) fn start(
    inner: Weak<DiscoveryInner>,
) -> Result<(Backend, JoinHandle<Result<(), DomainError>>), DomainError> {
    let backend = Backend {
        daemon: ServiceDaemon::new().map_err(discovery_error)?,
    };
    // The existing HTTPS listener and return-address selection use IPv4.
    backend
        .daemon
        .disable_interface(vec![IfKind::IPv6, IfKind::LoopbackV4])
        .map_err(discovery_error)?;
    let events = backend
        .daemon
        .browse(&format!("{SERVICE_TYPE}.local."))
        .map_err(discovery_error)?;
    let monitor = backend.daemon.monitor().map_err(discovery_error)?;
    let receiver = tokio::spawn(async move {
        loop {
            tokio::select! {
                result = events.recv_async() => {
                    match result.map_err(discovery_error)? {
                        ServiceEvent::ServiceResolved(service) => {
                            let device = decode_device(service.get_port(), |key| service.get_property_val(key).flatten());
                            match device {
                                Ok(device) => update_peer(&inner, service.get_fullname(), Some((device, service.get_addresses_v4().into_iter().collect()))),
                                Err(error) => {
                                    update_peer(&inner, service.get_fullname(), None);
                                    tracing::warn!("Ignoring invalid LAN mDNS service: {error}");
                                }
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, name) => update_peer(&inner, &name, None),
                        _ => {}
                    }
                }
                result = monitor.recv_async() => {
                    if let DaemonEvent::Error(error) = result.map_err(discovery_error)? {
                        // One interface failing does not make devices on other interfaces unusable.
                        tracing::warn!("LAN mDNS discovery: {error}");
                    }
                }
            }
        }
    });
    Ok((backend, receiver))
}

impl Backend {
    pub(super) async fn announce(
        &mut self,
        device: &LanDiscoveryAnnouncement,
    ) -> Result<(), DomainError> {
        let properties = txt_properties(device)
            .iter()
            .map(|(key, value)| TxtProperty::from((*key, value.as_slice())))
            .collect::<Vec<_>>();
        let service = ServiceInfo::new(
            &format!("{SERVICE_TYPE}.local."),
            device.device_id.as_str(),
            &format!("tt-{}.local.", device.device_id.as_str()),
            (),
            device.port,
            properties,
        )
        .map_err(discovery_error)?
        .enable_addr_auto();
        self.daemon.register(service).map_err(discovery_error)
    }

    pub(super) async fn stop(&mut self) -> Result<(), DomainError> {
        self.daemon
            .shutdown()
            .map_err(discovery_error)?
            .recv_async()
            .await
            .map_err(discovery_error)?;
        Ok(())
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
