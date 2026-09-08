use async_trait::async_trait;
use tt_contracts::lan_discovery::LanDiscoveredDevice;
use tt_domain::errors::DomainError;

#[async_trait]
pub trait LanDeviceDiscovery: Send + Sync {
    async fn discover_devices(&self) -> Result<Vec<LanDiscoveredDevice>, DomainError>;
    async fn set_device_name(&self, name: &str) -> Result<(), DomainError>;
}

#[async_trait]
pub trait LanDiscoveryHost: Send + Sync {
    /// Acquire or release host resources required for multicast reception.
    async fn set_enabled(&self, enabled: bool) -> Result<(), DomainError>;

    fn publish_changed(&self, devices: Vec<LanDiscoveredDevice>);
}
