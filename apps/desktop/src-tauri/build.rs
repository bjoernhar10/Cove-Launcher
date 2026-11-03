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
    tauri_build::build();
}
