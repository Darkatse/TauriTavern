use std::collections::HashMap;
use std::sync::Arc;

use tt_domain::errors::DomainError;
pub use tt_ports::bundled_template::BundledTemplateStore;

#[derive(Clone)]
pub struct BundledTemplateService {
    store: Arc<dyn BundledTemplateStore>,
    cache: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl BundledTemplateService {
    pub fn new<S>(store: Arc<S>) -> Self
    where
        S: BundledTemplateStore + 'static,
    {
        let store: Arc<dyn BundledTemplateStore> = store;
        Self {
            store,
            cache: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn read_frontend_template(&self, name: &str) -> Result<String, DomainError> {
        validate_resource_segment(name, "template name")?;

        let resource_path = format!("frontend-templates/{name}");
        self.read_resource_text(&resource_path)
            .map_err(|error| wrap_template_read_error(name, error))
    }

    pub fn read_frontend_extension_template(
        &self,
        extension: &str,
        name: &str,
    ) -> Result<String, DomainError> {
        validate_resource_segment(extension, "extension")?;
        validate_resource_segment(name, "template name")?;

        let resource_path = format!("frontend-extensions/{extension}/{name}.html");
        self.read_resource_text(&resource_path)
            .map_err(|error| wrap_extension_template_read_error(&resource_path, error))
    }

    fn read_resource_text(&self, resource_path: &str) -> Result<String, DomainError> {
        if let Some(content) = self
            .cache
            .lock()
            .expect("bundled template cache lock poisoned")
            .get(resource_path)
            .cloned()
        {
            return Ok(content);
        }

        let content = self.store.read_text(resource_path)?;
        self.cache
            .lock()
            .expect("bundled template cache lock poisoned")
            .insert(resource_path.to_string(), content.clone());
        Ok(content)
    }
}

fn validate_resource_segment(value: &str, field: &str) -> Result<(), DomainError> {
    if value.is_empty() || value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(DomainError::InvalidData(format!(
            "Invalid {field}: {value}"
        )));
    }
    Ok(())
}

fn wrap_template_read_error(name: &str, error: DomainError) -> DomainError {
    match error {
        DomainError::NotFound(message) => DomainError::NotFound(message),
        other => DomainError::InternalError(format!("Failed to read template '{name}': {other}")),
    }
}

fn wrap_extension_template_read_error(resource_path: &str, error: DomainError) -> DomainError {
    match error {
        DomainError::NotFound(message) => DomainError::NotFound(message),
        other => DomainError::InternalError(format!(
            "Failed to read extension template '{resource_path}': {other}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Store {
        paths: Mutex<Vec<String>>,
        text: &'static str,
    }

    impl Store {
        fn new(text: &'static str) -> Arc<Self> {
            Arc::new(Self {
                paths: Mutex::new(Vec::new()),
                text,
            })
        }

        fn paths(&self) -> Vec<String> {
            self.paths.lock().expect("paths lock poisoned").clone()
        }
    }

    impl BundledTemplateStore for Store {
        fn read_text(&self, relative_path: &str) -> Result<String, DomainError> {
            self.paths
                .lock()
                .expect("paths lock poisoned")
                .push(relative_path.to_string());

            Ok(self.text.to_string())
        }
    }

    #[test]
    fn reads_frontend_template_from_bundled_resource_path() {
        let store = Store::new("template");
        let service = BundledTemplateService::new(store.clone());

        let content = service.read_frontend_template("drawer.html").unwrap();

        assert_eq!(content, "template");
        assert_eq!(store.paths(), vec!["frontend-templates/drawer.html"]);
    }

    #[test]
    fn rejects_path_segments_before_reading_store() {
        let store = Store::new("unused");
        let service = BundledTemplateService::new(store.clone());

        let error = service
            .read_frontend_template("../drawer.html")
            .unwrap_err();

        assert!(
            matches!(error, DomainError::InvalidData(message) if message == "Invalid template name: ../drawer.html")
        );
        assert!(store.paths().is_empty());
    }
}
