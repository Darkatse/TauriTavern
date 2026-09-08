use std::collections::{HashMap, HashSet};
use std::sync::Weak;
use std::time::Duration;

use async_dnssd::{
    BrowseData, BrowseResult, BrowsedFlags, RegisterData, Registration, ResolveResult,
    ResolvedHostFlags, ScopedSocketAddr, TxtRecord,
};
use futures_util::future::{AbortHandle, Abortable};
use futures_util::stream::FuturesUnordered;
use futures_util::{StreamExt, TryStreamExt};
use tokio::task::JoinHandle;
use tt_contracts::lan_discovery::LanDiscoveryAnnouncement;
use tt_domain::errors::DomainError;

use super::{
    DiscoveryInner, MAX_DEVICES, SERVICE_TYPE, decode_device, discovery_error, txt_properties,
    update_peer,
};

pub(super) struct Backend {
    registration: Option<(u16, Registration)>,
}

pub(super) fn start(
    inner: Weak<DiscoveryInner>,
) -> Result<(Backend, JoinHandle<Result<(), DomainError>>), DomainError> {
    Ok((Backend { registration: None }, tokio::spawn(browse(inner))))
}

impl Backend {
    pub(super) async fn announce(
        &mut self,
        device: &LanDiscoveryAnnouncement,
    ) -> Result<(), DomainError> {
        let mut txt = TxtRecord::new();
        for (key, value) in txt_properties(device) {
            txt.set_value(key.as_bytes(), &value)
                .map_err(|error| discovery_error(format!("{error:?}")))?;
        }
        if let Some((port, registration)) = &self.registration
            && *port == device.port
        {
            return registration
                .get_default_txt_record()
                .update_record(txt.rdata(), 0)
                .map_err(discovery_error);
        }
        self.registration = None;
        let registering = async_dnssd::register_extended(
            SERVICE_TYPE,
            device.port,
            RegisterData {
                name: Some(device.device_id.as_str()),
                domain: Some("local."),
                txt: txt.rdata(),
                ..Default::default()
            },
        )
        .map_err(discovery_error)?;
        // A permission prompt or failed registration must not hold up the HTTPS server.
        let (registration, _) = tokio::time::timeout(Duration::from_secs(3), registering)
            .await
            .map_err(|_| discovery_error("Timed out announcing this device"))?
            .map_err(discovery_error)?;
        self.registration = Some((device.port, registration));
        Ok(())
    }

    pub(super) async fn stop(&mut self) -> Result<(), DomainError> {
        self.registration = None;
        Ok(())
    }
}

async fn browse(inner: Weak<DiscoveryInner>) -> Result<(), DomainError> {
    let mut browser = async_dnssd::browse_extended(
        SERVICE_TYPE,
        BrowseData {
            domain: Some("local."),
            ..Default::default()
        },
    );
    let mut handles = HashMap::<String, AbortHandle>::new();
    let mut services = FuturesUnordered::new();
    loop {
        tokio::select! {
            result = browser.try_next() => {
                let service = result.map_err(discovery_error)?
                    .ok_or_else(|| discovery_error("Bonjour browser stopped"))?;
                let source = format!("{}@{}", service.service_name, service.interface.into_raw());
                if !service.flags.contains(BrowsedFlags::ADD) {
                    if let Some(handle) = handles.remove(&source) {
                        handle.abort();
                    }
                    update_peer(&inner, &source, None);
                } else if !handles.contains_key(&source) && handles.len() < MAX_DEVICES {
                    let (handle, registration) = AbortHandle::new_pair();
                    handles.insert(source.clone(), handle);
                    let inner = inner.clone();
                    services.push(Abortable::new(async move {
                        let result = watch_service(service, &source, &inner).await;
                        (source, result)
                    }, registration));
                }
            }
            Some(result) = services.next(), if !services.is_empty() => {
                if let Ok((source, result)) = result {
                    handles.remove(&source);
                    update_peer(&inner, &source, None);
                    if let Err(error) = result {
                        tracing::warn!("Cannot resolve Bonjour service {source}: {error}");
                    }
                }
            }
        }
    }
}

async fn watch_service(
    service: BrowseResult,
    source: &str,
    inner: &Weak<DiscoveryInner>,
) -> Result<(), DomainError> {
    let mut resolving = service.resolve();
    let mut resolved = resolving
        .try_next()
        .await
        .map_err(discovery_error)?
        .ok_or_else(|| discovery_error("Bonjour service resolution stopped"))?;
    let mut device = decode_resolved(&resolved)?;
    let mut addresses = resolved.resolve_socket_address();
    let mut ips = HashSet::new();
    loop {
        tokio::select! {
            result = resolving.try_next() => {
                let next = result.map_err(discovery_error)?
                    .ok_or_else(|| discovery_error("Bonjour service resolution stopped"))?;
                device = decode_resolved(&next)?;
                if next.host_target != resolved.host_target || next.port != resolved.port {
                    ips.clear();
                    addresses = next.resolve_socket_address();
                }
                resolved = next;
            }
            result = addresses.try_next() => {
                let address = result.map_err(discovery_error)?
                    .ok_or_else(|| discovery_error("Bonjour address resolution stopped"))?;
                let ScopedSocketAddr::V4 { address: ip, .. } = address.address else { continue };
                if address.flags.contains(ResolvedHostFlags::ADD) {
                    ips.insert(ip);
                } else {
                    ips.remove(&ip);
                }
            }
        }
        update_peer(
            inner,
            source,
            Some((device.clone(), ips.iter().copied().collect())),
        );
    }
}

fn decode_resolved(service: &ResolveResult) -> Result<LanDiscoveryAnnouncement, DomainError> {
    let txt = TxtRecord::parse(&service.txt)
        .ok_or_else(|| DomainError::InvalidData("Invalid Bonjour TXT record".to_string()))?;
    decode_device(service.port, |key| txt.get(key.as_bytes()).flatten())
}
