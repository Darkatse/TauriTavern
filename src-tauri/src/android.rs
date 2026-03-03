use jni::objects::JObject;
use jni::JNIEnv;

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn Java_com_tauritavern_client_MainActivity_initRustlsPlatformVerifier<'local>(
    mut env: JNIEnv<'local>,
    _activity: JObject<'local>,
    context: JObject<'local>,
) {
    rustls_platform_verifier::android::init_with_env(&mut env, context)
        .expect("Failed to initialize rustls-platform-verifier on Android");
}
