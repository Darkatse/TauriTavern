use tokio::sync::{mpsc::UnboundedSender, watch};

/// Upstream stream event payloads in order. A `[DONE]` payload ends the generation stream.
pub type GenerationStreamSender = UnboundedSender<String>;
pub type GenerationCancelReceiver = watch::Receiver<bool>;
