pub trait UserEndpointGrantRuntime: Send + Sync {
    fn replace_user_endpoint_grants(&self, endpoints: &[String]);
}
