use std::sync::Arc;

use crate::domain::errors::DomainError;
pub(crate) use tt_ports::bundled_template::BundledTemplateStore;

#[derive(Clone)]
pub(crate) struct BundledTemplateService {
    store: Arc<dyn BundledTemplateStore>,
}

impl BundledTemplateService {
    pub(crate) fn new<S>(store: Arc<S>) -> Self
    where
        S: BundledTemplateStore + 'static,
    {
        let store: Arc<dyn BundledTemplateStore> = store;
        Self { store }
    }

    pub(crate) fn read_frontend_template(&self, name: &str) -> Result<String, DomainError> {
        validate_resource_segment(name, "template name")?;

        let resource_path = format!("frontend-templates/{name}");
        self.store
            .read_text(&resource_path)
            .map_err(|error| wrap_template_read_error(name, error))
    }

    pub(crate) fn read_frontend_extension_template(
        &self,
        extension: &str,
        name: &str,
    ) -> Result<String, DomainError> {
        validate_resource_segment(extension, "extension")?;
        validate_resource_segment(name, "template name")?;

        let resource_path = format!("frontend-extensions/{extension}/{name}.html");
        self.store
            .read_text(&resource_path)
            .map_err(|error| wrap_extension_template_read_error(&resource_path, error))
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

    #[derive(Clone, Copy)]
    enum StoreResult {
        Text(&'static str),
        NotFound(&'static str),
        Invalid(&'static str),
    }

    struct Store {
        paths: Mutex<Vec<String>>,
        result: StoreResult,
    }

    impl Store {
        fn new(result: StoreResult) -> Arc<Self> {
            Arc::new(Self {
                paths: Mutex::new(Vec::new()),
                result,
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

            match self.result {
                StoreResult::Text(text) => Ok(text.to_string()),
                StoreResult::NotFound(message) => Err(DomainError::NotFound(message.to_string())),
                StoreResult::Invalid(message) => Err(DomainError::InvalidData(message.to_string())),
            }
        }
    }

    #[test]
    fn reads_frontend_template_from_bundled_resource_path() {
        let store = Store::new(StoreResult::Text("template"));
        let service = BundledTemplateService::new(store.clone());

        let content = service.read_frontend_template("drawer.html").unwrap();

        assert_eq!(content, "template");
        assert_eq!(store.paths(), vec!["frontend-templates/drawer.html"]);
    }

    #[test]
    fn reads_extension_template_from_bundled_resource_path() {
        let store = Store::new(StoreResult::Text("extension"));
        let service = BundledTemplateService::new(store.clone());

        let content = service
            .read_frontend_extension_template("quick-replies", "button")
            .unwrap();

        assert_eq!(content, "extension");
        assert_eq!(
            store.paths(),
            vec!["frontend-extensions/quick-replies/button.html"]
        );
    }

    #[test]
    fn rejects_path_segments_before_reading_store() {
        let store = Store::new(StoreResult::Text("unused"));
        let service = BundledTemplateService::new(store.clone());

        let error = service
            .read_frontend_template("../drawer.html")
            .unwrap_err();

        assert!(
            matches!(error, DomainError::InvalidData(message) if message == "Invalid template name: ../drawer.html")
        );
        assert!(store.paths().is_empty());
    }

    #[test]
    fn passes_not_found_through() {
        let store = Store::new(StoreResult::NotFound("Resource not found: missing"));
        let service = BundledTemplateService::new(store);

        let error = service.read_frontend_template("missing.html").unwrap_err();

        assert!(
            matches!(error, DomainError::NotFound(message) if message == "Resource not found: missing")
        );
    }

    #[test]
    fn wraps_non_not_found_errors_with_template_context() {
        let store = Store::new(StoreResult::Invalid("not utf-8"));
        let service = BundledTemplateService::new(store);

        let error = service.read_frontend_template("broken.html").unwrap_err();

        assert!(matches!(
            error,
            DomainError::InternalError(message)
                if message == "Failed to read template 'broken.html': Invalid data: not utf-8"
        ));
    }
}
