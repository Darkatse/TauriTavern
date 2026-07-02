use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const BACKEND_ERROR_EVENT: &str = "tauritavern-backend-error";

const MAX_PENDING_BACKEND_ERRORS: usize = 50;

#[derive(Clone, Serialize)]
struct BackendErrorEventPayload {
    message: String,
}

pub struct BackendErrorHub {
    app_handle: AppHandle,
    state: Mutex<BackendErrorState>,
}

impl BackendErrorHub {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            state: Mutex::new(BackendErrorState::default()),
        }
    }

    pub fn emit_or_queue(&self, message: String) {
        let normalized = message.trim();
        if normalized.is_empty() {
            return;
        }
        let message = normalized.to_string();

        {
            let mut state = self.state.lock().unwrap();
            if !state.bridge_ready {
                state.push_pending(message);
                return;
            }
        }

        self.emit(message);
    }

    pub fn mark_bridge_ready_and_drain(&self) -> Vec<String> {
        self.state.lock().unwrap().mark_ready_and_drain()
    }

    fn emit(&self, message: String) {
        if let Err(error) = self
            .app_handle
            .emit(BACKEND_ERROR_EVENT, BackendErrorEventPayload { message })
        {
            eprintln!("Failed to emit backend error event: {}", error);
        }
    }
}

#[derive(Default)]
struct BackendErrorState {
    bridge_ready: bool,
    pending: VecDeque<String>,
}

impl BackendErrorState {
    fn push_pending(&mut self, message: String) {
        self.pending.push_back(message);
        while self.pending.len() > MAX_PENDING_BACKEND_ERRORS {
            self.pending.pop_front();
        }
    }

    fn mark_ready_and_drain(&mut self) -> Vec<String> {
        self.bridge_ready = true;
        self.pending.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_error_state_drains_pending_errors_once() {
        let mut state = BackendErrorState::default();
        state.push_pending("first".to_string());
        state.push_pending("second".to_string());

        assert_eq!(
            state.mark_ready_and_drain(),
            vec!["first".to_string(), "second".to_string()]
        );
        assert!(state.mark_ready_and_drain().is_empty());
        assert!(state.bridge_ready);
    }

    #[test]
    fn backend_error_state_caps_pending_errors() {
        let mut state = BackendErrorState::default();
        for index in 0..(MAX_PENDING_BACKEND_ERRORS + 2) {
            state.push_pending(index.to_string());
        }

        let pending = state.mark_ready_and_drain();
        assert_eq!(pending.len(), MAX_PENDING_BACKEND_ERRORS);
        assert_eq!(pending.first().map(String::as_str), Some("2"));
    }
}
