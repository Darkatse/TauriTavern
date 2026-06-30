fn main() {
    println!("cargo:rerun-if-env-changed=TAURITAVERN_IOS_POLICY_PROFILE");

    let target = std::env::var("TARGET").unwrap_or_default();
    let is_ios_target = target.contains("-apple-ios");
    let raw_profile = std::env::var("TAURITAVERN_IOS_POLICY_PROFILE").unwrap_or_default();
    let profile = if is_ios_target {
        raw_profile.trim()
    } else {
        ""
    };

    if is_ios_target && !profile.is_empty() {
        match profile {
            "full" | "ios_internal_full" | "ios_external_beta" => {}
            value => {
                panic!(
                    "TAURITAVERN_IOS_POLICY_PROFILE has unsupported value {value:?}. Expected one of: full, ios_internal_full, ios_external_beta."
                );
            }
        }
    }

    println!("cargo:rustc-env=TAURITAVERN_IOS_POLICY_PROFILE={profile}");
}
