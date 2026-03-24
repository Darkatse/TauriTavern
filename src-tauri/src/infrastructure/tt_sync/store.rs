use std::path::PathBuf;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use uuid::Uuid;

use crate::domain::errors::DomainError;
use crate::domain::models::tt_sync::{TtSyncIdentity, TtSyncPairedServer};
use crate::infrastructure::persistence::file_system::{read_json_file, write_json_file};

pub struct TtSyncStore {
    tt_sync_dir: PathBuf,
}

impl TtSyncStore {
    pub fn new(default_user_dir: PathBuf) -> Self {
        Self {
            tt_sync_dir: default_user_dir
                .join("user")
                .join("lan-sync")
                .join("tt-sync-v2"),
        }
    }

    fn identity_path(&self) -> PathBuf {
        self.tt_sync_dir.join("identity.json")
    }

    fn paired_servers_path(&self) -> PathBuf {
        self.tt_sync_dir.join("paired-servers.json")
    }

    pub async fn load_or_create_identity(&self) -> Result<TtSyncIdentity, DomainError> {
        let path = self.identity_path();
        if path.is_file() {
            return read_json_file(&path).await;
        }

        let identity = TtSyncIdentity {
            device_id: ttsync_contract::peer::DeviceId::new(Uuid::new_v4().to_string())
                .expect("generated uuid must be valid"),
            device_name: "TauriTavern".to_string(),
            ed25519_seed: random_seed_b64url(),
        };
        write_json_file(&path, &identity).await?;
        Ok(identity)
    }

    pub async fn load_paired_servers(&self) -> Result<Vec<TtSyncPairedServer>, DomainError> {
        let path = self.paired_servers_path();
        if !path.is_file() {
            return Ok(Vec::new());
        }
        read_json_file(&path).await
    }

    pub async fn save_paired_servers(
        &self,
        servers: &[TtSyncPairedServer],
    ) -> Result<(), DomainError> {
        let path = self.paired_servers_path();
        write_json_file(&path, servers).await
    }

    pub async fn upsert_paired_server(
        &self,
        server: TtSyncPairedServer,
    ) -> Result<(), DomainError> {
        let mut servers = self.load_paired_servers().await?;

        if let Some(existing) = servers
            .iter_mut()
            .find(|item| item.server_device_id == server.server_device_id)
        {
            *existing = server;
        } else {
            servers.push(server);
        }

        self.save_paired_servers(&servers).await
    }

    pub async fn remove_paired_server(
        &self,
        server_device_id: &ttsync_contract::peer::DeviceId,
    ) -> Result<(), DomainError> {
        let servers = self.load_paired_servers().await?;
        let filtered = servers
            .into_iter()
            .filter(|server| &server.server_device_id != server_device_id)
            .collect::<Vec<_>>();

        self.save_paired_servers(&filtered).await
    }
}

fn random_seed_b64url() -> String {
    let mut seed = [0u8; 32];
    rand::rng().fill_bytes(&mut seed);
    URL_SAFE_NO_PAD.encode(seed)
}
