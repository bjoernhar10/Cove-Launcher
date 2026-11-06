//! Local image cache behind the `cove-img://` URI scheme.
//!
//! Catalogue artwork is served to the webview as `cove-img://localhost/<url-encoded remote url>`
//! (Windows: `http://cove-img.localhost/…`). The first request downloads the file into the app's
//! cache folder; later requests — also offline — are answered from disk.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use sha2::{Digest, Sha256};

pub const SCHEME: &str = "cove-img";
pub const ALLOWED_HOSTS: &[&str] = &[
    "images.igdb.com",
    "media.rawg.io",
    // Steam game icons and achievement icons
    "media.steampowered.com",
    "steamcdn-a.akamaihd.net",
    "cdn.akamai.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "shared.akamai.steamstatic.com",
    // itch.io cover images
    "img.itch.zone",
    // GOG product images
    "images.gog.com",
    "images.gog-statics.com",
    "images-1.gog-statics.com",
    "images-2.gog-statics.com",
    "images-3.gog-statics.com",
    "images-4.gog-statics.com",
];
const MAX_BYTES: usize = 15 * 1024 * 1024;

/// URL the webview loads for a remote image.
pub fn proxy_url(remote: &str) -> String {
    let encoded = utf8_percent_encode(remote, NON_ALPHANUMERIC);
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/{encoded}")
    } else {
        format!("{SCHEME}://localhost/{encoded}")
    }
}

fn proxy_prefix() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/")
    } else {
        format!("{SCHEME}://localhost/")
    }
}

/// Rewrites remote http(s) URLs to the local proxy; leaves everything else untouched.
pub fn proxied(url: Option<String>) -> Option<String> {
    url.map(|u| {
        let remote = (u.starts_with("https://") || u.starts_with("http://"))
            && !u.starts_with(&proxy_prefix());
        if remote {
            proxy_url(&u)
        } else {
            u
        }
    })
}

/// Inverse of [`proxied`]: restores the remote URL of a proxy URL.
pub fn unproxied(url: Option<String>) -> Option<String> {
    url.map(|u| match u.strip_prefix(&proxy_prefix()) {
        Some(rest) => ImageCache::decode_request_path(rest).unwrap_or(u),
        None => u,
    })
}

/// Steam's portrait library artwork (`library_600x900_2x.jpg`) is missing for some games,
/// and newer games keep all their artwork under hashed paths. Fallbacks, best first:
/// the 1x portrait, the landscape header, then the header URL the store API names.
const STEAM_PORTRAIT: &str = "/library_600x900_2x.jpg";
const STEAM_API: &str = "https://store.steampowered.com";

fn steam_app(remote: &str) -> Option<(&str, &str)> {
    let base = remote.strip_suffix(STEAM_PORTRAIT)?;
    let (_, app_id) = base.rsplit_once("/steam/apps/")?;
    (!app_id.is_empty() && app_id.bytes().all(|b| b.is_ascii_digit())).then_some((base, app_id))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ImageError {
    #[error("ungültige Bild-URL")]
    BadRequest,
    #[error("Host nicht erlaubt")]
    Forbidden,
    #[error("nicht im Cache und keine Verbindung")]
    Unavailable,
    #[error("Bild zu groß")]
    TooLarge,
}

pub struct ImageCache {
    dir: PathBuf,
    client: reqwest::Client,
    allowed_hosts: Vec<String>,
    steam_api: String,
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/jpeg",
    }
}

impl ImageCache {
    pub fn new(dir: PathBuf, client: reqwest::Client) -> Self {
        Self {
            dir,
            client,
            allowed_hosts: ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
            steam_api: STEAM_API.to_owned(),
        }
    }

    #[cfg(test)]
    pub fn with_hosts(dir: PathBuf, client: reqwest::Client, hosts: &[&str]) -> Self {
        Self {
            dir,
            client,
            allowed_hosts: hosts.iter().map(|h| (*h).to_owned()).collect(),
            steam_api: STEAM_API.to_owned(),
        }
    }

    /// Remote URL from the request path of a `cove-img` request.
    pub fn decode_request_path(path: &str) -> Option<String> {
        let decoded = percent_decode_str(path.trim_start_matches('/'))
            .decode_utf8()
            .ok()?
            .into_owned();
        (decoded.starts_with("https://") || decoded.starts_with("http://")).then_some(decoded)
    }

    fn check(&self, remote: &str) -> Result<reqwest::Url, ImageError> {
        let url = reqwest::Url::parse(remote).map_err(|_| ImageError::BadRequest)?;
        let host = url.host_str().ok_or(ImageError::BadRequest)?;
        if !self.allowed_hosts.iter().any(|h| h == host) {
            return Err(ImageError::Forbidden);
        }
        Ok(url)
    }

    pub fn path_for(&self, remote: &str) -> PathBuf {
        let hash = Sha256::digest(remote.as_bytes());
        let hex = hash.iter().fold(String::with_capacity(64), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        });
        let ext = Path::new(remote.split(['?', '#']).next().unwrap_or(remote))
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| {
                ["jpg", "jpeg", "png", "webp", "gif"].contains(&e.to_ascii_lowercase().as_str())
            })
            .unwrap_or("jpg")
            .to_ascii_lowercase();
        self.dir.join(format!("{hex}.{ext}"))
    }

    /// Bytes + MIME type, from disk or downloaded (and stored) on first use.
    pub async fn get(&self, remote: &str) -> Result<(Vec<u8>, &'static str), ImageError> {
        let url = self.check(remote)?;
        let path = self.path_for(remote);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            return Ok((bytes, mime_for(&path)));
        }
        let bytes = match (self.download(url).await, steam_app(remote)) {
            (Err(ImageError::Unavailable), Some((base, app_id))) => {
                self.steam_fallback(base, app_id).await?
            }
            (result, _) => result?,
        };
        // Write atomically so a crash never leaves a truncated image behind.
        if tokio::fs::create_dir_all(&self.dir).await.is_ok() {
            let tmp = path.with_extension("part");
            if tokio::fs::write(&tmp, &bytes).await.is_ok() {
                let _ = tokio::fs::rename(&tmp, &path).await;
            }
        }
        Ok((bytes, mime_for(&path)))
    }

    async fn steam_fallback(&self, base: &str, app_id: &str) -> Result<Vec<u8>, ImageError> {
        for file in ["library_600x900.jpg", "header.jpg"] {
            if let Ok(bytes) = self.download(self.check(&format!("{base}/{file}"))?).await {
                return Ok(bytes);
            }
        }
        let details = self
            .client
            .get(format!("{}/api/appdetails", self.steam_api))
            .query(&[("appids", app_id), ("filters", "basic")])
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| ImageError::Unavailable)?
            .json::<serde_json::Value>()
            .await
            .map_err(|_| ImageError::Unavailable)?;
        let header = details[app_id]["data"]["header_image"]
            .as_str()
            .ok_or(ImageError::Unavailable)?;
        self.download(self.check(header)?).await
    }

    async fn download(&self, url: reqwest::Url) -> Result<Vec<u8>, ImageError> {
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| ImageError::Unavailable)?;
        if !response.status().is_success() {
            return Err(ImageError::Unavailable);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| ImageError::Unavailable)?;
        if bytes.len() > MAX_BYTES {
            return Err(ImageError::TooLarge);
        }
        Ok(bytes.to_vec())
    }

    /// Downloads images in the background so they are available offline.
    pub async fn prefetch(&self, urls: &[String]) {
        for url in urls {
            let _ = self.get(url).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn proxy_urls_roundtrip() {
        let remote = "https://images.igdb.com/igdb/image/upload/t_cover_big/co1wyy.jpg";
        let local = proxy_url(remote);
        assert!(
            local.starts_with("cove-img://localhost/")
                || local.starts_with("http://cove-img.localhost/")
        );
        let request_path = local.split_once("localhost").unwrap().1;
        assert_eq!(
            ImageCache::decode_request_path(request_path).as_deref(),
            Some(remote)
        );
        assert_eq!(
            proxied(Some("asset://x".into())).as_deref(),
            Some("asset://x")
        );
        assert_eq!(proxied(None), None);
        assert_eq!(unproxied(Some(local.clone())).as_deref(), Some(remote));
        assert_eq!(
            proxied(Some(local.clone())).as_deref(),
            Some(local.as_str()),
            "idempotent"
        );
        assert_eq!(unproxied(Some(remote.into())).as_deref(), Some(remote));
    }

    #[test]
    fn image_refs_are_proxied_on_the_wire_only() {
        let remote = "https://media.rawg.io/media/a.jpg";
        let image = crate::models::ImageRef {
            url: Some(remote.into()),
            alt: "a".into(),
        };
        let json = serde_json::to_value(&image).unwrap();
        assert!(json["url"].as_str().unwrap().contains("cove-img"));
        let back: crate::models::ImageRef = serde_json::from_value(json).unwrap();
        assert_eq!(back, image);
    }

    #[tokio::test]
    async fn downloads_once_then_serves_from_disk() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cover.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PNGDATA".to_vec()))
            .expect(1)
            .mount(&server)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let cache = ImageCache::with_hosts(
            tmp.path().join("img"),
            reqwest::Client::new(),
            &["127.0.0.1"],
        );
        let url = format!("{}/cover.png", server.uri());

        let (bytes, mime) = cache.get(&url).await.unwrap();
        assert_eq!(
            (bytes.as_slice(), mime),
            (b"PNGDATA".as_slice(), "image/png")
        );
        assert!(cache.path_for(&url).is_file());

        drop(server); // offline now
        let (again, _) = cache.get(&url).await.unwrap();
        assert_eq!(again, b"PNGDATA");
    }

    #[tokio::test]
    async fn steam_portrait_falls_back_step_by_step() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/steam/apps/10/header.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"HEADER".to_vec()))
            .mount(&server)
            .await;
        // A newer game: only the store API knows its hashed header path.
        Mock::given(method("GET"))
            .and(path("/api/appdetails"))
            .and(wiremock::matchers::query_param("appids", "20"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "20": { "data": { "header_image": format!("{}/steam/apps/20/ab12/header.jpg?t=1", server.uri()) } }
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/steam/apps/20/ab12/header.jpg"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"HASHED".to_vec()))
            .mount(&server)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let mut cache = ImageCache::with_hosts(
            tmp.path().join("img"),
            reqwest::Client::new(),
            &["127.0.0.1"],
        );
        cache.steam_api = server.uri();
        let portrait =
            |id: &str| format!("{}/steam/apps/{id}/library_600x900_2x.jpg", server.uri());
        assert_eq!(cache.get(&portrait("10")).await.unwrap().0, b"HEADER");
        assert_eq!(cache.get(&portrait("20")).await.unwrap().0, b"HASHED");
        assert!(
            cache.path_for(&portrait("20")).is_file(),
            "cached under the portrait URL"
        );
        assert_eq!(
            cache.get(&portrait("30")).await.unwrap_err(),
            ImageError::Unavailable
        );
        let other = format!("{}/steam/apps/10/other.jpg", server.uri());
        assert_eq!(
            cache.get(&other).await.unwrap_err(),
            ImageError::Unavailable
        );
    }

    #[tokio::test]
    async fn rejects_foreign_hosts_and_reports_offline_misses() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = ImageCache::new(tmp.path().to_path_buf(), reqwest::Client::new());
        assert_eq!(
            cache.get("https://evil.example/x.jpg").await.unwrap_err(),
            ImageError::Forbidden
        );
        assert_eq!(
            cache.get("not a url").await.unwrap_err(),
            ImageError::BadRequest
        );

        let local = ImageCache::with_hosts(
            tmp.path().to_path_buf(),
            reqwest::Client::new(),
            &["127.0.0.1"],
        );
        assert_eq!(
            local
                .get("http://127.0.0.1:9/missing.jpg")
                .await
                .unwrap_err(),
            ImageError::Unavailable
        );
    }
}
