//! Login in an own Tauri window: the provider's official page is shown and
//! every navigation is checked. When the browser is about to load the
//! redirect target, the code is taken from the URL and the window closes.
//!
//! Epic shows the code in the *body* of its redirect page (JSON). A small
//! script turns it into a navigation to `cove-auth://code?value=…`, which is
//! intercepted the same way, so the page never needs IPC access.

use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::mpsc;

use super::loopback::parse_query;
use crate::error::{AppError, AppResult};

pub const CODE_SCHEME_URL: &str = "cove-auth://code";

/// What to do with a URL the login window wants to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Navigation {
    /// Let the page load (login pages, 2FA, …).
    Allow,
    /// The code arrived: stop and close.
    Code(String),
    /// The redirect page whose body holds the code (Epic): load it, then read the body.
    ReadBody,
    /// The provider reported an error or the user aborted.
    Failed(String),
}

/// Pure decision used by the navigation handler (unit-tested).
pub fn classify(url: &str, callback_prefix: &str) -> Navigation {
    if let Some(query) = url.strip_prefix(CODE_SCHEME_URL) {
        let params = parse_query(query.trim_start_matches('?'));
        return match params.get("value").map(String::as_str) {
            Some(code) if !code.is_empty() => Navigation::Code(code.to_owned()),
            _ => Navigation::Failed("Der Anbieter hat keinen Anmeldecode geliefert.".into()),
        };
    }
    if !url.starts_with(callback_prefix) {
        return Navigation::Allow;
    }
    let params = url
        .split_once('?')
        .map(|(_, q)| parse_query(q))
        .unwrap_or_default();
    if let Some(code) = params.get("code").filter(|c| !c.is_empty()) {
        return Navigation::Code(code.clone());
    }
    if let Some(error) = params.get("error") {
        return Navigation::Failed(format!("Anmeldung abgelehnt: {error}"));
    }
    Navigation::ReadBody
}

/// Script for the redirect page: read `authorizationCode` from the JSON body and navigate to the code URL.
pub fn read_body_script() -> String {
    format!(
        "(function(){{try{{var d=JSON.parse(document.body.innerText);\
         var c=d.authorizationCode||d.code||'';window.location.href='{CODE_SCHEME_URL}?value='+encodeURIComponent(c);}}\
         catch(e){{window.location.href='{CODE_SCHEME_URL}?value=';}}}})();"
    )
}

/// Opens the login window and resolves with the authorization code.
pub async fn login_in_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    title: &str,
    url: &str,
    callback_prefix: &str,
    timeout: Duration,
) -> AppResult<String> {
    let target: tauri::Url = url
        .parse()
        .map_err(|_| AppError::LoginFailed("Ungültige Anmeldeadresse.".into()))?;
    if let Some(existing) = app.get_webview_window(label) {
        let _ = existing.destroy();
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<Result<String, String>>();
    let nav_tx = tx.clone();
    let prefix = callback_prefix.to_owned();
    let load_prefix = callback_prefix.to_owned();
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::External(target))
        .title(title)
        .inner_size(520.0, 720.0)
        .center()
        .on_navigation(move |next| match classify(next.as_str(), &prefix) {
            Navigation::Allow | Navigation::ReadBody => true,
            Navigation::Code(code) => {
                let _ = nav_tx.send(Ok(code));
                false
            }
            Navigation::Failed(message) => {
                let _ = nav_tx.send(Err(message));
                false
            }
        })
        .on_page_load(move |webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && classify(payload.url().as_str(), &load_prefix) == Navigation::ReadBody
            {
                let _ = webview.eval(read_body_script());
            }
        })
        .build()
        .map_err(|e| {
            AppError::LoginFailed(format!("Anmeldefenster konnte nicht geöffnet werden: {e}"))
        })?;

    let closed_tx = tx;
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let _ = closed_tx.send(Err("Die Anmeldung wurde abgebrochen.".into()));
        }
    });

    let result = tokio::time::timeout(timeout, rx.recv()).await;
    let _ = window.destroy();
    match result {
        Ok(Some(Ok(code))) => Ok(code),
        Ok(Some(Err(message))) => Err(AppError::LoginFailed(message)),
        Ok(None) => Err(AppError::LoginFailed(
            "Die Anmeldung wurde abgebrochen.".into(),
        )),
        Err(_) => Err(AppError::LoginFailed(
            "Die Anmeldung wurde nicht rechtzeitig abgeschlossen.".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOG: &str = "https://embed.gog.com/on_login_success";
    const EPIC: &str = "https://www.epicgames.com/id/api/redirect";

    #[test]
    fn gog_code_is_read_from_the_redirect_url() {
        assert_eq!(
            classify("https://login.gog.com/login?x=1", GOG),
            Navigation::Allow
        );
        assert_eq!(
            classify(
                "https://embed.gog.com/on_login_success?origin=client&code=abc123",
                GOG
            ),
            Navigation::Code("abc123".into())
        );
        assert!(matches!(
            classify(
                "https://embed.gog.com/on_login_success?error=access_denied",
                GOG
            ),
            Navigation::Failed(_)
        ));
        // A look-alike host must not match.
        assert_eq!(
            classify(
                "https://embed.gog.com.evil.test/on_login_success?code=x",
                GOG
            ),
            Navigation::Allow
        );
    }

    #[test]
    fn epic_code_comes_from_the_page_body() {
        assert_eq!(
            classify("https://www.epicgames.com/id/login?redirectUrl=x", EPIC),
            Navigation::Allow
        );
        assert_eq!(
            classify(
                "https://www.epicgames.com/id/api/redirect?clientId=a&responseType=code",
                EPIC
            ),
            Navigation::ReadBody
        );
        assert_eq!(
            classify("cove-auth://code?value=f00d", EPIC),
            Navigation::Code("f00d".into())
        );
        assert!(matches!(
            classify("cove-auth://code?value=", EPIC),
            Navigation::Failed(_)
        ));
        let script = read_body_script();
        assert!(script.contains("authorizationCode") && script.contains("cove-auth://code?value="));
    }
}
