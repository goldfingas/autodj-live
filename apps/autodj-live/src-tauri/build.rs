fn main() {
    let profile = std::env::var("PROFILE").expect("Cargo must provide PROFILE");
    println!("cargo:rustc-env=AUTODJ_BUILD_PROFILE={profile}");
    if profile != "release" && std::env::var_os("TAURI_CONFIG").is_none() {
        // Development uses explicit path overrides and does not require staged bundle artifacts.
        unsafe {
            std::env::set_var("TAURI_CONFIG", r#"{"bundle":{"resources":[]}}"#);
        }
    }
    tauri_build::build();
}
