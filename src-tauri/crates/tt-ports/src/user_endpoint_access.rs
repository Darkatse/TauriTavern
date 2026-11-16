pub trait UserEndpointGrantRuntime: Send + Sync {
    /// `origins` are written by `endpoint_url::http_origin`.
    fn allow_user_endpoints(&self, origins: &[String]);
}
