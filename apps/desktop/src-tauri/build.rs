fn main() {
    // Base URL of the Cove catalogue proxy (services/catalog-proxy), baked in at build time:
    //   COVE_CATALOG_PROXY_URL=https://cove-catalog-proxy.<account>.workers.dev npm run tauri:build
    // Empty → the proxy is disabled and only own API keys work.
    println!("cargo:rerun-if-env-changed=COVE_CATALOG_PROXY_URL");
    let url = std::env::var("COVE_CATALOG_PROXY_URL").unwrap_or_default();
    println!(
        "cargo:rustc-env=COVE_CATALOG_PROXY_URL={}",
        url.trim().trim_end_matches('/')
    );
    ensure_sidecar_placeholder("cove-recommender", "build:recommender");
    ensure_sidecar_placeholder("cove-connectors", "build:connectors");
    embed_windows_manifest();
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    if let Err(error) =
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
    {
        panic!("tauri build failed: {error:#}");
    }
}

/// Windows: Tauri only embeds its app manifest (Common Controls v6) into the
/// app binary, so `cargo test` binaries die at start with
/// `STATUS_ENTRYPOINT_NOT_FOUND`. A linker argument puts the same manifest into
/// every binary of the crate, tests included.
fn embed_windows_manifest() {
    let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
        .join("windows-app-manifest.xml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    if windows && msvc {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}

/// Tauri refuses to build when an `externalBin` is missing. Without a built
/// sidecar (`npm run build:sidecar -w @cove/desktop`) we put a small stand-in
/// there; the app detects it and reports the service as unavailable (in
/// debug builds it falls back to the service in the repository).
fn ensure_sidecar_placeholder(name: &str, script: &str) {
    let triple = std::env::var("TARGET").unwrap_or_default();
    let exe = if triple.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path = std::path::Path::new("binaries").join(format!("{name}-{triple}{exe}"));
    println!("cargo:rerun-if-changed={}", path.display());
    if path.exists() {
        return;
    }
    let _ = std::fs::create_dir_all("binaries");
    let body = format!(
        "#!/bin/sh\n# cove-sidecar-placeholder: run `npm run {script} -w @cove/desktop`\n\
         echo '{name} is not built' >&2\nexit 3\n"
    );
    if std::fs::write(&path, body).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
}
