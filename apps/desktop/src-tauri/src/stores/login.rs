//! Interactive login flows. Steam: system browser + RFC 8252 loopback
//! redirect with a `state` check. GOG/Epic: the official page in an own
//! window whose navigation is intercepted. The result is reported as the
//! `connector-login-finished` event; a flow can be cancelled at any time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::oneshot;

use super::StoreService;
use crate::auth::loopback::LoopbackServer;
use crate::auth::state::new_state;
use crate::auth::webview::login_in_window;
use crate::error::{AppError, AppErrorPayload, AppResult};
use crate::models::{ConnectorLoginFinished, ConnectorLoginMode, ConnectorLoginStart, StoreSource};

pub const LOGIN_FINISHED: &str = "connector-login-finished";
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Running login flows (one per store) and their cancel switches.
#[derive(Default)]
pub struct LoginFlows {
    pending: Mutex<HashMap<StoreSource, oneshot::Sender<()>>>,
}

impl LoginFlows {
    fn register(&self, store: StoreSource) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        let previous = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(store, tx);
        if let Some(previous) = previous {
            let _ = previous.send(());
        }
        rx
    }

    /// Stops a running flow; true if there was one.
    pub fn cancel(&self, store: StoreSource) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&store)
            .is_some_and(|tx| tx.send(()).is_ok())
    }

    fn finish(&self, store: StoreSource) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&store);
    }
}

pub fn window_label(store: StoreSource) -> String {
    format!("login-{}", store.as_str())
}

pub type OpenUrl = Arc<dyn Fn(&str) -> AppResult<()> + Send + Sync>;
pub type OnLogin = Arc<dyn Fn(StoreSource) + Send + Sync>;

/// Starts the login and returns at once; the outcome arrives as an event.
pub async fn start<R: Runtime>(
    app: &AppHandle<R>,
    stores: Arc<StoreService>,
    flows: Arc<LoginFlows>,
    store: StoreSource,
    open_url: OpenUrl,
    on_login: OnLogin,
) -> AppResult<ConnectorLoginStart> {
    let state = new_state()?;
    let mode = crate::db::sources::login_mode(store);
    let app = app.clone();
    match mode {
        ConnectorLoginMode::Browser => {
            let server = LoopbackServer::bind().await?;
            let redirect = server.redirect_uri();
            let begin = stores.login_start(store, &redirect, &state).await?;
            open_url(&begin.url)?;
            let cancel = flows.register(store);
            let expected_return_to = format!("{redirect}?state={state}");
            tauri::async_runtime::spawn(async move {
                let outcome = tokio::select! {
                    callback = server.wait(&state, LOGIN_TIMEOUT) => Some(match callback {
                        Ok(callback) => stores
                            .login_complete(store, Some(&callback.url), None, Some(&expected_return_to))
                            .await,
                        Err(error) => Err(error),
                    }),
                    _ = cancel => None,
                };
                report(&app, &flows, store, outcome, &on_login);
            });
            Ok(ConnectorLoginStart {
                store,
                mode,
                fallback_url: begin.fallback_url,
            })
        }
        ConnectorLoginMode::Webview => {
            let begin = stores.login_start(store, "", &state).await?;
            let prefix = begin.callback_prefix.clone().ok_or_else(|| {
                AppError::StoreUnavailable(tr!(
                    "{}-Login ist falsch konfiguriert.",
                    "The {} sign-in is configured incorrectly.",
                    store.display_name()
                ))
            })?;
            let cancel = flows.register(store);
            let label = window_label(store);
            let title = tr!("Bei {} anmelden", "Sign in to {}", store.display_name());
            let url = begin.url.clone();
            tauri::async_runtime::spawn(async move {
                let outcome = tokio::select! {
                    code = login_in_window(&app, &label, &title, &url, &prefix, LOGIN_TIMEOUT) => Some(match code {
                        Ok(code) => stores.login_complete(store, None, Some(&code), None).await,
                        Err(error) => Err(error),
                    }),
                    _ = cancel => None,
                };
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.destroy();
                }
                report(&app, &flows, store, outcome, &on_login);
            });
            Ok(ConnectorLoginStart {
                store,
                mode,
                fallback_url: begin.fallback_url,
            })
        }
    }
}

/// `None` = cancelled (the cancel command reports that itself).
fn report<R: Runtime>(
    app: &AppHandle<R>,
    flows: &LoginFlows,
    store: StoreSource,
    outcome: Option<AppResult<crate::models::StoreAccount>>,
    on_login: &OnLogin,
) {
    let Some(outcome) = outcome else { return };
    flows.finish(store);
    let event = match outcome {
        Ok(account) => {
            on_login(store);
            ConnectorLoginFinished {
                store,
                account: Some(account),
                error: None,
            }
        }
        Err(error) => {
            eprintln!("[cove] {}-Login: {error}", store.display_name());
            ConnectorLoginFinished {
                store,
                account: None,
                error: Some(AppErrorPayload::from(&error)),
            }
        }
    };
    let _ = app.emit(LOGIN_FINISHED, event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_flow_cancels_the_previous_one() {
        let flows = LoginFlows::default();
        let mut first = flows.register(StoreSource::Gog);
        let _second = flows.register(StoreSource::Gog);
        assert!(first.try_recv().is_ok(), "first flow was told to stop");
        assert!(flows.cancel(StoreSource::Gog));
        assert!(!flows.cancel(StoreSource::Gog), "nothing left to cancel");
        assert!(!flows.cancel(StoreSource::Epic));
    }
}
