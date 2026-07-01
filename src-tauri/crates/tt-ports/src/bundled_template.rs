use tt_domain::errors::DomainError;

pub trait BundledTemplateStore: Send + Sync {
    fn read_text(&self, relative_path: &str) -> Result<String, DomainError>;
}
