use tokio::sync::Mutex;
use ttsync_contract::sync::SyncMode;

use tt_domain::errors::DomainError;

#[derive(Debug, Clone)]
pub struct LanPairingSession {
    pub token: String,
    pub expires_at_ms: u64,
}

pub struct LanSyncRuntimeState {
    pairing_session: Mutex<Option<LanPairingSession>>,
    sync_mode_override: Mutex<Option<SyncMode>>,
}

impl LanSyncRuntimeState {
    pub fn new() -> Self {
        Self {
            pairing_session: Mutex::new(None),
            sync_mode_override: Mutex::new(None),
        }
    }

    pub async fn set_pairing_session(&self, session: LanPairingSession) {
        *self.pairing_session.lock().await = Some(session);
    }

    pub async fn get_pairing_session(&self) -> Option<LanPairingSession> {
        self.pairing_session.lock().await.clone()
    }

    pub async fn clear_pairing_session(&self) {
        *self.pairing_session.lock().await = None;
    }

    pub async fn active_pairing_session(
        &self,
        token: &str,
        now_ms: u64,
    ) -> Result<LanPairingSession, DomainError> {
        let session =
            self.pairing_session.lock().await.clone().ok_or_else(|| {
                DomainError::AuthenticationError("Pairing not enabled".to_string())
            })?;
        validate_pairing_session(&session, token, now_ms)?;
        Ok(session)
    }

    pub async fn consume_pairing_session(
        &self,
        token: &str,
        now_ms: u64,
    ) -> Result<(), DomainError> {
        let mut pairing_session = self.pairing_session.lock().await;
        let session = pairing_session
            .as_ref()
            .ok_or_else(|| DomainError::AuthenticationError("Pairing not enabled".to_string()))?;
        validate_pairing_session(session, token, now_ms)?;
        *pairing_session = None;
        Ok(())
    }

    pub async fn get_sync_mode_override(&self) -> Option<SyncMode> {
        *self.sync_mode_override.lock().await
    }

    pub async fn set_sync_mode_override(&self, mode: Option<SyncMode>) {
        *self.sync_mode_override.lock().await = mode;
    }
}

impl Default for LanSyncRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_pairing_session(
    session: &LanPairingSession,
    token: &str,
    now_ms: u64,
) -> Result<(), DomainError> {
    if token != session.token {
        return Err(DomainError::AuthenticationError(
            "Invalid pairing token".to_string(),
        ));
    }
    if now_ms > session.expires_at_ms {
        return Err(DomainError::AuthenticationError(
            "Pairing expired".to_string(),
        ));
    }
    Ok(())
}
