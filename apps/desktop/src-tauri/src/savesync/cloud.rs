//! The user's own cloud for save sync: Google Drive (its hidden app data
//! folder, scope `drive.appdata`), `OneDrive` (its app folder, scope
//! `Files.ReadWrite.AppFolder`) and Dropbox (an "App folder" app). Cove sees
//! only that folder, never the user's other files. Sign-in runs in the system
//! browser with PKCE and a loopback redirect (Dropbox wants the exact address,
//! so it gets a fixed port); Google wants its client secret too, so Google's
//! token exchange goes through Cove's proxy, which holds that secret. The
//! refresh token lives only in the OS keyring.

use std::path::Path;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};
use crate::models::CloudProvider;

/// The keyring entry of the connected cloud.
pub const SESSION_KEY: &str = "save-sync-session";

/// Uploads go in pieces of this size (a multiple of 320 KiB and 256 KiB, as
/// `OneDrive` and Google want).
const CHUNK: usize = 10 * 1024 * 1024;
/// `OneDrive` takes files up to 4 MiB in one request.
const ONEDRIVE_SIMPLE: u64 = 4 * 1024 * 1024;

pub const GOOGLE_CLIENT_ID: &str = match option_env!("COVE_GOOGLE_CLIENT_ID") {
    Some(id) => id,
    None => "",
};
pub const ONEDRIVE_CLIENT_ID: &str = match option_env!("COVE_ONEDRIVE_CLIENT_ID") {
    Some(id) => id,
    None => "",
};
pub const DROPBOX_CLIENT_ID: &str = match option_env!("COVE_DROPBOX_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// Dropbox only takes redirect addresses registered in full, port included.
pub const DROPBOX_PORT: u16 = 53682;
/// Dropbox's API arguments travel in a header (`Dropbox-API-Arg`).
const DROPBOX_ARG: &str = "Dropbox-API-Arg";

const GOOGLE_SCOPE: &str = "https://www.googleapis.com/auth/drive.appdata";
const ONEDRIVE_SCOPE: &str = "Files.ReadWrite.AppFolder User.Read offline_access";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub google_auth: String,
    /// Drive API and upload host (`/drive/v3`, `/upload/drive/v3`).
    pub google_api: String,
    /// Cove's proxy (Google's token exchange); None = Google is not offered.
    pub proxy: Option<String>,
    /// Microsoft identity platform (`…/authorize`, `…/token`).
    pub microsoft_login: String,
    pub graph: String,
    pub google_client_id: String,
    pub onedrive_client_id: String,
    pub dropbox_auth: String,
    /// RPC host (`/oauth2/token`, `/2/files/delete_v2` …).
    pub dropbox_api: String,
    /// Content host (`/2/files/download`, `/2/files/upload` …).
    pub dropbox_content: String,
    pub dropbox_client_id: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            google_auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            google_api: "https://www.googleapis.com".into(),
            proxy: crate::catalog::proxy::build_time_url(),
            microsoft_login: "https://login.microsoftonline.com/common/oauth2/v2.0".into(),
            graph: "https://graph.microsoft.com/v1.0".into(),
            google_client_id: GOOGLE_CLIENT_ID.into(),
            onedrive_client_id: ONEDRIVE_CLIENT_ID.into(),
            dropbox_auth: "https://www.dropbox.com/oauth2/authorize".into(),
            dropbox_api: "https://api.dropboxapi.com".into(),
            dropbox_content: "https://content.dropboxapi.com".into(),
            dropbox_client_id: DROPBOX_CLIENT_ID.into(),
        }
    }
}

impl Endpoints {
    /// Clouds this build can sign in to.
    pub fn available(&self) -> Vec<CloudProvider> {
        let mut out = Vec::new();
        if !self.google_client_id.is_empty() && self.proxy.is_some() {
            out.push(CloudProvider::GoogleDrive);
        }
        if !self.onedrive_client_id.is_empty() {
            out.push(CloudProvider::OneDrive);
        }
        if !self.dropbox_client_id.is_empty() {
            out.push(CloudProvider::Dropbox);
        }
        out
    }
}

pub fn provider_name(provider: CloudProvider) -> &'static str {
    match provider {
        CloudProvider::GoogleDrive => "Google Drive",
        CloudProvider::OneDrive => "OneDrive",
        CloudProvider::Dropbox => "Dropbox",
    }
}

/// What the keyring keeps about the connected cloud.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub provider: CloudProvider,
    pub refresh_token: String,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub needs_reauth: bool,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("provider", &self.provider)
            .field("refresh_token", &"<redacted>")
            .field("account", &self.account)
            .field("needs_reauth", &self.needs_reauth)
            .finish()
    }
}

/// PKCE verifier (43 characters) and its S256 challenge.
pub fn pkce() -> AppResult<(String, String)> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| AppError::LoginFailed(e.to_string()))?;
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Ok((verifier, challenge))
}

fn query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            format!(
                "{k}={}",
                percent_encoding::utf8_percent_encode(v, percent_encoding::NON_ALPHANUMERIC)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// The sign-in page in the browser.
pub fn authorize_url(
    endpoints: &Endpoints,
    provider: CloudProvider,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    match provider {
        CloudProvider::GoogleDrive => format!(
            "{}?{}",
            endpoints.google_auth,
            query(&[
                ("client_id", &endpoints.google_client_id),
                ("redirect_uri", redirect_uri),
                ("response_type", "code"),
                ("scope", GOOGLE_SCOPE),
                ("state", state),
                ("code_challenge", challenge),
                ("code_challenge_method", "S256"),
                ("access_type", "offline"),
                ("prompt", "consent"),
            ])
        ),
        CloudProvider::OneDrive => format!(
            "{}/authorize?{}",
            endpoints.microsoft_login,
            query(&[
                ("client_id", &endpoints.onedrive_client_id),
                ("redirect_uri", redirect_uri),
                ("response_type", "code"),
                ("scope", ONEDRIVE_SCOPE),
                ("state", state),
                ("code_challenge", challenge),
                ("code_challenge_method", "S256"),
                ("prompt", "select_account"),
            ])
        ),
        CloudProvider::Dropbox => format!(
            "{}?{}",
            endpoints.dropbox_auth,
            query(&[
                ("client_id", &endpoints.dropbox_client_id),
                ("redirect_uri", redirect_uri),
                ("response_type", "code"),
                ("token_access_type", "offline"),
                ("state", state),
                ("code_challenge", challenge),
                ("code_challenge_method", "S256"),
            ])
        ),
    }
}

pub enum Grant<'a> {
    Code {
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
    },
    Refresh(&'a str),
}

#[derive(Debug)]
pub struct Token {
    pub access: String,
    /// A new refresh token (`OneDrive` rotates them; Google sends one at sign-in).
    pub refresh: Option<String>,
    pub expires_in: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProxyToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct MicrosoftToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

fn unreachable_cloud(provider: CloudProvider) -> AppError {
    let name = provider_name(provider);
    AppError::StoreUnavailable(tr!(
        "{name} ist gerade nicht erreichbar.",
        "{name} can't be reached right now."
    ))
}

/// Trades a sign-in code or a refresh token for an access token.
#[allow(clippy::too_many_lines)] // one request and one answer per cloud
pub async fn exchange(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    provider: CloudProvider,
    grant: Grant<'_>,
) -> AppResult<Token> {
    let response = match provider {
        CloudProvider::GoogleDrive => {
            let proxy = endpoints
                .proxy
                .as_deref()
                .ok_or_else(|| unreachable_cloud(provider))?;
            let body = match grant {
                Grant::Code {
                    code,
                    verifier,
                    redirect_uri,
                } => serde_json::json!({
                    "grantType": "authorization_code",
                    "code": code,
                    "codeVerifier": verifier,
                    "redirectUri": redirect_uri,
                }),
                Grant::Refresh(token) => {
                    serde_json::json!({ "grantType": "refresh_token", "refreshToken": token })
                }
            };
            client
                .post(format!(
                    "{}/v1/oauth/google/token",
                    proxy.trim_end_matches('/')
                ))
                .json(&body)
                .send()
                .await
        }
        CloudProvider::OneDrive | CloudProvider::Dropbox => {
            let (url, mut form) = if provider == CloudProvider::OneDrive {
                (
                    format!("{}/token", endpoints.microsoft_login),
                    vec![
                        ("client_id", endpoints.onedrive_client_id.as_str()),
                        ("scope", ONEDRIVE_SCOPE),
                    ],
                )
            } else {
                (
                    format!("{}/oauth2/token", endpoints.dropbox_api),
                    vec![("client_id", endpoints.dropbox_client_id.as_str())],
                )
            };
            match grant {
                Grant::Code {
                    code,
                    verifier,
                    redirect_uri,
                } => form.extend([
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("code_verifier", verifier),
                    ("redirect_uri", redirect_uri),
                ]),
                Grant::Refresh(token) => {
                    form.extend([("grant_type", "refresh_token"), ("refresh_token", token)]);
                }
            }
            client
                .post(url)
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(query(&form))
                .send()
                .await
        }
    }
    .map_err(|_| unreachable_cloud(provider))?;
    let status = response.status();
    if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
        eprintln!(
            "[cove] {} lehnt die Anmeldung ab: HTTP {status}",
            provider_name(provider)
        );
        return Err(AppError::ReauthRequired(provider_name(provider).into()));
    }
    if !status.is_success() {
        return Err(unreachable_cloud(provider));
    }
    match provider {
        CloudProvider::GoogleDrive => {
            let token: ProxyToken = response
                .json()
                .await
                .map_err(|_| unreachable_cloud(provider))?;
            Ok(Token {
                access: token.access_token,
                refresh: token.refresh_token,
                expires_in: token.expires_in,
            })
        }
        CloudProvider::OneDrive | CloudProvider::Dropbox => {
            let token: MicrosoftToken = response
                .json()
                .await
                .map_err(|_| unreachable_cloud(provider))?;
            Ok(Token {
                access: token.access_token,
                refresh: token.refresh_token,
                expires_in: token.expires_in.unwrap_or(3600),
            })
        }
    }
}

/// The account's e-mail or name, to show which account is connected.
pub async fn account_name(
    client: &reqwest::Client,
    endpoints: &Endpoints,
    provider: CloudProvider,
    access: &str,
) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GoogleUser {
        email_address: Option<String>,
        display_name: Option<String>,
    }
    #[derive(Deserialize)]
    struct GoogleAbout {
        user: GoogleUser,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct MicrosoftMe {
        user_principal_name: Option<String>,
        display_name: Option<String>,
    }
    #[derive(Deserialize)]
    struct DropboxName {
        display_name: Option<String>,
    }
    #[derive(Deserialize)]
    struct DropboxAccount {
        email: Option<String>,
        name: Option<DropboxName>,
    }
    if provider == CloudProvider::Dropbox {
        // An RPC call without arguments: the body is JSON `null`.
        let response = client
            .post(format!(
                "{}/2/users/get_current_account",
                endpoints.dropbox_api
            ))
            .bearer_auth(access)
            .header("Content-Type", "application/json")
            .body("null")
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let account: DropboxAccount = response.json().await.ok()?;
        return account
            .email
            .or_else(|| account.name.and_then(|n| n.display_name));
    }
    let url = match provider {
        CloudProvider::GoogleDrive => format!(
            "{}/drive/v3/about?fields=user(displayName,emailAddress)",
            endpoints.google_api
        ),
        CloudProvider::OneDrive | CloudProvider::Dropbox => format!(
            "{}/me?$select=displayName,userPrincipalName",
            endpoints.graph
        ),
    };
    let response = client.get(url).bearer_auth(access).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    match provider {
        CloudProvider::GoogleDrive => {
            let about: GoogleAbout = response.json().await.ok()?;
            about.user.email_address.or(about.user.display_name)
        }
        CloudProvider::OneDrive | CloudProvider::Dropbox => {
            let me: MicrosoftMe = response.json().await.ok()?;
            me.user_principal_name.or(me.display_name)
        }
    }
}

/// The connected cloud's folder, with a fresh access token. Names are paths
/// with `/` below that folder (Google keeps them as plain file names).
pub struct Remote {
    client: reqwest::Client,
    /// Upload sessions answer 308 while incomplete: never follow redirects.
    upload: reqwest::Client,
    endpoints: Endpoints,
    provider: CloudProvider,
    access: String,
}

#[derive(Deserialize)]
struct DriveFile {
    id: String,
}

#[derive(Deserialize)]
struct DriveList {
    #[serde(default)]
    files: Vec<DriveFile>,
}

#[derive(Deserialize)]
struct DropboxStarted {
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadSession {
    upload_url: String,
}

impl Remote {
    pub fn new(
        client: reqwest::Client,
        upload: reqwest::Client,
        endpoints: Endpoints,
        provider: CloudProvider,
        access: String,
    ) -> Self {
        Self {
            client,
            upload,
            endpoints,
            provider,
            access,
        }
    }

    fn failed(&self, status: reqwest::StatusCode) -> AppError {
        eprintln!(
            "[cove] {} antwortet mit HTTP {status}",
            provider_name(self.provider)
        );
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return AppError::ReauthRequired(provider_name(self.provider).into());
        }
        if status == reqwest::StatusCode::INSUFFICIENT_STORAGE {
            let name = provider_name(self.provider);
            return AppError::StoreUnavailable(tr!(
                "In deinem {name} ist kein Platz mehr frei.",
                "Your {name} is full."
            ));
        }
        unreachable_cloud(self.provider)
    }

    fn offline(&self) -> AppError {
        unreachable_cloud(self.provider)
    }

    fn onedrive_item(&self, name: &str) -> String {
        let path = name
            .split('/')
            .map(|part| {
                percent_encoding::utf8_percent_encode(part, percent_encoding::NON_ALPHANUMERIC)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("/");
        format!("{}/me/drive/special/approot:/{path}:", self.endpoints.graph)
    }

    /// Google: the ids of the files with this name in the app data folder.
    async fn drive_ids(&self, name: &str) -> AppResult<Vec<String>> {
        let escaped = name.replace('\\', "\\\\").replace('\'', "\\'");
        let response = self
            .client
            .get(format!("{}/drive/v3/files", self.endpoints.google_api))
            .query(&[
                ("spaces", "appDataFolder"),
                ("q", &format!("name = '{escaped}' and trashed = false")),
                ("fields", "files(id)"),
                ("pageSize", "100"),
            ])
            .bearer_auth(&self.access)
            .send()
            .await
            .map_err(|_| self.offline())?;
        if !response.status().is_success() {
            return Err(self.failed(response.status()));
        }
        let list: DriveList = response.json().await.map_err(|_| self.offline())?;
        Ok(list.files.into_iter().map(|f| f.id).collect())
    }

    async fn open(&self, name: &str) -> AppResult<Option<reqwest::Response>> {
        let request = match self.provider {
            CloudProvider::GoogleDrive => {
                let Some(id) = self.drive_ids(name).await?.into_iter().next() else {
                    return Ok(None);
                };
                self.client.get(format!(
                    "{}/drive/v3/files/{id}?alt=media",
                    self.endpoints.google_api
                ))
            }
            CloudProvider::OneDrive => self
                .client
                .get(format!("{}/content", self.onedrive_item(name))),
            CloudProvider::Dropbox => self
                .client
                .post(format!(
                    "{}/2/files/download",
                    self.endpoints.dropbox_content
                ))
                .header(
                    DROPBOX_ARG,
                    dropbox_arg(&serde_json::json!({ "path": dropbox_path(name) })),
                ),
        };
        let response = request
            .bearer_auth(&self.access)
            .send()
            .await
            .map_err(|_| self.offline())?;
        // Dropbox answers a missing file with 409 `path/not_found`.
        if response.status() == reqwest::StatusCode::NOT_FOUND
            || (self.provider == CloudProvider::Dropbox
                && response.status() == reqwest::StatusCode::CONFLICT)
        {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(self.failed(response.status()));
        }
        Ok(Some(response))
    }

    /// A small file (the manifest); None when it isn't there.
    pub async fn get(&self, name: &str) -> AppResult<Option<Vec<u8>>> {
        let Some(response) = self.open(name).await? else {
            return Ok(None);
        };
        Ok(Some(
            response.bytes().await.map_err(|_| self.offline())?.to_vec(),
        ))
    }

    /// Downloads into `to`; returns the SHA-256 (hex) of what arrived.
    pub async fn download(&self, name: &str, to: &Path) -> AppResult<Option<String>> {
        let Some(mut response) = self.open(name).await? else {
            return Ok(None);
        };
        let mut file = tokio::fs::File::create(to).await?;
        let mut hash = Sha256::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| self.offline())? {
            hash.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(Some(hex(&hash.finalize())))
    }

    /// Writes a file. `replace`: an existing one of that name gets the new
    /// content (Google allows several files with one name).
    pub async fn put_bytes(&self, name: &str, bytes: Vec<u8>, replace: bool) -> AppResult<()> {
        let size = bytes.len() as u64;
        self.put(name, size, replace, Source::Bytes(bytes)).await
    }

    pub async fn put_file(&self, name: &str, path: &Path) -> AppResult<()> {
        let size = tokio::fs::metadata(path).await?.len();
        self.put(name, size, false, Source::File(path)).await
    }

    async fn put(&self, name: &str, size: u64, replace: bool, source: Source<'_>) -> AppResult<()> {
        match self.provider {
            CloudProvider::GoogleDrive => {
                let existing = if replace {
                    self.drive_ids(name).await?.into_iter().next()
                } else {
                    None
                };
                let request = match &existing {
                    Some(id) => self
                        .client
                        .patch(format!(
                            "{}/upload/drive/v3/files/{id}?uploadType=resumable",
                            self.endpoints.google_api
                        ))
                        .json(&serde_json::json!({})),
                    None => self
                        .client
                        .post(format!(
                            "{}/upload/drive/v3/files?uploadType=resumable",
                            self.endpoints.google_api
                        ))
                        .json(&serde_json::json!({ "name": name, "parents": ["appDataFolder"] })),
                };
                let response = request
                    .bearer_auth(&self.access)
                    .header("X-Upload-Content-Length", size)
                    .send()
                    .await
                    .map_err(|_| self.offline())?;
                if !response.status().is_success() {
                    return Err(self.failed(response.status()));
                }
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| self.offline())?
                    .to_owned();
                self.send_chunks(&location, size, source, true).await
            }
            CloudProvider::OneDrive if size <= ONEDRIVE_SIMPLE => {
                let bytes = source.all().await?;
                let response = self
                    .client
                    .put(format!("{}/content", self.onedrive_item(name)))
                    .bearer_auth(&self.access)
                    .header("Content-Type", "application/octet-stream")
                    .body(bytes)
                    .send()
                    .await
                    .map_err(|_| self.offline())?;
                if !response.status().is_success() {
                    return Err(self.failed(response.status()));
                }
                Ok(())
            }
            CloudProvider::Dropbox => self.dropbox_upload(name, size, source).await,
            CloudProvider::OneDrive => {
                let response = self
                    .client
                    .post(format!("{}/createUploadSession", self.onedrive_item(name)))
                    .bearer_auth(&self.access)
                    .json(&serde_json::json!({
                        "item": { "@microsoft.graph.conflictBehavior": "replace" }
                    }))
                    .send()
                    .await
                    .map_err(|_| self.offline())?;
                if !response.status().is_success() {
                    return Err(self.failed(response.status()));
                }
                let session: UploadSession = response.json().await.map_err(|_| self.offline())?;
                // The upload URL carries its own authorization.
                self.send_chunks(&session.upload_url, size, source, false)
                    .await
            }
        }
    }

    /// The content of an upload session, in pieces of `CHUNK`.
    async fn send_chunks(
        &self,
        url: &str,
        size: u64,
        source: Source<'_>,
        auth: bool,
    ) -> AppResult<()> {
        let mut reader = source.reader().await?;
        let mut offset = 0u64;
        loop {
            let mut piece = Vec::with_capacity(CHUNK.min(usize::try_from(size).unwrap_or(CHUNK)));
            (&mut reader)
                .take(CHUNK as u64)
                .read_to_end(&mut piece)
                .await?;
            let len = piece.len() as u64;
            if len == 0 && size > 0 {
                // The file got shorter while it was read.
                return Err(AppError::InvalidInput(tr!(
                    "Ein Spielstand hat sich beim Hochladen geändert.",
                    "A save changed while it was uploaded."
                )));
            }
            let mut request = self.upload.put(url).header("Content-Length", len);
            if size > 0 {
                request = request.header(
                    "Content-Range",
                    format!("bytes {offset}-{}/{size}", offset + len - 1),
                );
            }
            if auth {
                request = request.bearer_auth(&self.access);
            }
            let response = request
                .body(piece)
                .send()
                .await
                .map_err(|_| self.offline())?;
            offset += len;
            let status = response.status().as_u16();
            match status {
                200 | 201 => return Ok(()),
                // Google: 308 resume incomplete; OneDrive: 202 accepted.
                202 | 308 if offset < size => {}
                _ => return Err(self.failed(response.status())),
            }
        }
    }

    /// Dropbox: one request up to `CHUNK`, else an upload session
    /// (start, append, finish with the commit).
    async fn dropbox_upload(&self, name: &str, size: u64, source: Source<'_>) -> AppResult<()> {
        let commit =
            serde_json::json!({ "path": dropbox_path(name), "mode": "overwrite", "mute": true });
        let content = &self.endpoints.dropbox_content;
        let send = |url: String, arg: serde_json::Value, body: Vec<u8>| {
            self.client
                .post(url)
                .bearer_auth(&self.access)
                .header(DROPBOX_ARG, dropbox_arg(&arg))
                .header("Content-Type", "application/octet-stream")
                .body(body)
                .send()
        };
        let check = |response: reqwest::Response| {
            if response.status().is_success() {
                Ok(response)
            } else {
                Err(self.failed(response.status()))
            }
        };
        if size <= CHUNK as u64 {
            let body = source.all().await?;
            check(
                send(format!("{content}/2/files/upload"), commit, body)
                    .await
                    .map_err(|_| self.offline())?,
            )?;
            return Ok(());
        }
        let mut reader = source.reader().await?;
        let mut offset = 0u64;
        let mut session: Option<String> = None;
        loop {
            let mut piece = Vec::with_capacity(CHUNK);
            (&mut reader)
                .take(CHUNK as u64)
                .read_to_end(&mut piece)
                .await?;
            let len = piece.len() as u64;
            let last = offset + len >= size;
            let response = match &session {
                None => send(
                    format!("{content}/2/files/upload_session/start"),
                    serde_json::json!({ "close": false }),
                    piece,
                ),
                Some(id) if last => send(
                    format!("{content}/2/files/upload_session/finish"),
                    serde_json::json!({
                        "cursor": { "session_id": id, "offset": offset },
                        "commit": commit,
                    }),
                    piece,
                ),
                Some(id) => send(
                    format!("{content}/2/files/upload_session/append_v2"),
                    serde_json::json!({
                        "cursor": { "session_id": id, "offset": offset },
                        "close": false,
                    }),
                    piece,
                ),
            }
            .await
            .map_err(|_| self.offline())?;
            let response = check(response)?;
            if session.is_none() {
                let started: DropboxStarted = response.json().await.map_err(|_| self.offline())?;
                session = Some(started.session_id);
            } else if last {
                return Ok(());
            }
            if len == 0 {
                // The file got shorter while it was read.
                return Err(AppError::InvalidInput(tr!(
                    "Ein Spielstand hat sich beim Hochladen geändert.",
                    "A save changed while it was uploaded."
                )));
            }
            offset += len;
        }
    }

    /// Removes a file; one that isn't there is fine.
    pub async fn delete(&self, name: &str) -> AppResult<()> {
        let requests = match self.provider {
            CloudProvider::GoogleDrive => self
                .drive_ids(name)
                .await?
                .into_iter()
                .map(|id| {
                    self.client
                        .delete(format!("{}/drive/v3/files/{id}", self.endpoints.google_api))
                })
                .collect(),
            CloudProvider::OneDrive => vec![self.client.delete(self.onedrive_item(name))],
            CloudProvider::Dropbox => vec![self
                .client
                .post(format!("{}/2/files/delete_v2", self.endpoints.dropbox_api))
                .json(&serde_json::json!({ "path": dropbox_path(name) }))],
        };
        for request in requests {
            let response = request
                .bearer_auth(&self.access)
                .send()
                .await
                .map_err(|_| self.offline())?;
            let status = response.status();
            // Dropbox: 409 `path_lookup/not_found`.
            let missing = status == reqwest::StatusCode::NOT_FOUND
                || (self.provider == CloudProvider::Dropbox
                    && status == reqwest::StatusCode::CONFLICT);
            if !status.is_success() && !missing {
                return Err(self.failed(status));
            }
        }
        Ok(())
    }
}

enum Source<'a> {
    Bytes(Vec<u8>),
    File(&'a Path),
}

impl Source<'_> {
    async fn all(self) -> AppResult<Vec<u8>> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::File(path) => Ok(tokio::fs::read(path).await?),
        }
    }

    async fn reader(self) -> AppResult<Box<dyn tokio::io::AsyncRead + Unpin + Send>> {
        match self {
            Self::Bytes(bytes) => Ok(Box::new(std::io::Cursor::new(bytes))),
            Self::File(path) => Ok(Box::new(tokio::fs::File::open(path).await?)),
        }
    }
}

/// `cove-saves/v1/x` → `/cove-saves/v1/x` (Dropbox paths start at the app folder).
fn dropbox_path(name: &str) -> String {
    format!("/{name}")
}

/// The `Dropbox-API-Arg` header: JSON with everything outside ASCII escaped.
pub fn dropbox_arg(value: &serde_json::Value) -> String {
    value.to_string().chars().fold(String::new(), |mut out, c| {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{unit:04x}");
            }
        }
        out
    })
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// HTTP clients: one that follows redirects (`OneDrive` downloads), one that doesn't (uploads).
pub fn clients() -> (reqwest::Client, reqwest::Client) {
    let build = |redirects| {
        reqwest::Client::builder()
            .user_agent(concat!(
                "Cove/",
                env!("CARGO_PKG_VERSION"),
                " (desktop game launcher)"
            ))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .redirect(redirects)
            .build()
            .unwrap_or_default()
    };
    (
        build(reqwest::redirect::Policy::limited(5)),
        build(reqwest::redirect::Policy::none()),
    )
}
