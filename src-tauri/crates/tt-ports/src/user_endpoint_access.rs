pub trait UserEndpointGrantRuntime: Send + Sync {
    fn allow_user_endpoints(&self, endpoints: &[String]);
}
