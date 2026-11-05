//! RFC 8252 loopback redirect receiver: a one-shot HTTP listener on
//! 127.0.0.1 with a random port. It accepts exactly one request to
//! `/callback` whose `state` matches, answers with a small page that closes
//! itself, and hands the full callback URL to the caller.

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::state::constant_time_eq;
use crate::error::{AppError, AppResult};

const MAX_REQUEST_BYTES: usize = 16 * 1024;

pub struct LoopbackServer {
    listener: TcpListener,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    /// Full URL as the provider redirected to it (incl. query).
    pub url: String,
    pub params: HashMap<String, String>,
}

/// The small page the browser shows after the redirect, in the UI language.
fn page(title: &str, text: &str, close: bool) -> String {
    let lang = if crate::i18n::is_english() {
        "en"
    } else {
        "de"
    };
    let script = if close {
        "<script>setTimeout(function(){window.close()},800)</script>"
    } else {
        ""
    };
    format!(
        "<!doctype html><html lang=\"{lang}\"><meta charset=\"utf-8\"><title>Cove</title>\
<body style=\"font-family:system-ui,sans-serif;display:grid;place-items:center;height:90vh;color:#18181b\">\
<div style=\"text-align:center\"><h1 style=\"font-size:20px;font-weight:600\">{title}</h1>\
<p style=\"color:#5c5c66\">{text}</p></div>{script}</body></html>"
    )
}

fn done_page() -> String {
    page(
        &tr!("Angemeldet", "Signed in"),
        &tr!(
            "Du kannst dieses Fenster schließen und zu Cove zurückkehren.",
            "You can close this window and return to Cove."
        ),
        true,
    )
}

fn error_page() -> String {
    page(
        &tr!("Anmeldung nicht möglich", "Sign-in not possible"),
        &tr!(
            "Diese Antwort gehört nicht zu einer Anmeldung in Cove. Starte die Anmeldung in Cove erneut.",
            "This response doesn't belong to a sign-in from Cove. Start the sign-in in Cove again."
        ),
        false,
    )
}

fn decode(component: &str) -> String {
    percent_encoding::percent_decode_str(&component.replace('+', " "))
        .decode_utf8_lossy()
        .into_owned()
}

pub fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(pair), String::new()),
        })
        .collect()
}

impl LoopbackServer {
    /// Binds 127.0.0.1 on a random free port (never 0.0.0.0).
    pub async fn bind() -> AppResult<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Waits for the provider's redirect. Requests to other paths or with a
    /// wrong `state` are answered and ignored; the first valid one wins.
    pub async fn wait(self, expected_state: &str, timeout: Duration) -> AppResult<Callback> {
        let work = async {
            loop {
                let (stream, _) = self.listener.accept().await?;
                if let Some(callback) = Self::handle(stream, self.port, expected_state).await? {
                    return Ok::<Callback, AppError>(callback);
                }
            }
        };
        tokio::time::timeout(timeout, work).await.map_err(|_| {
            AppError::LoginFailed(tr!(
                "Die Anmeldung wurde nicht rechtzeitig abgeschlossen.",
                "The sign-in wasn't finished in time."
            ))
        })?
    }

    async fn handle(
        mut stream: TcpStream,
        port: u16,
        expected_state: &str,
    ) -> AppResult<Option<Callback>> {
        let mut buf = Vec::with_capacity(2048);
        let mut chunk = [0u8; 2048];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk)).await;
            let Ok(Ok(n)) = read else { return Ok(None) };
            if n == 0 || buf.len() + n > MAX_REQUEST_BYTES {
                return Ok(None);
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let head = String::from_utf8_lossy(&buf);
        let target = head
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|rest| rest.split(' ').next())
            .unwrap_or("");
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if path != "/callback" {
            Self::respond(&mut stream, "404 Not Found", "").await;
            return Ok(None);
        }
        let params = parse_query(query);
        let state_ok = params
            .get("state")
            .is_some_and(|s| constant_time_eq(s, expected_state));
        if !state_ok {
            Self::respond(&mut stream, "400 Bad Request", &error_page()).await;
            return Ok(None);
        }
        Self::respond(&mut stream, "200 OK", &done_page()).await;
        Ok(Some(Callback {
            url: format!("http://127.0.0.1:{port}{target}"),
            params,
        }))
    }

    async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
             Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream
            .write_all(
                format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn accepts_the_matching_callback_only() {
        let server = LoopbackServer::bind().await.unwrap();
        let redirect = server.redirect_uri();
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"));
        let port: u16 = redirect
            .trim_start_matches("http://127.0.0.1:")
            .trim_end_matches("/callback")
            .parse()
            .unwrap();
        let waiting =
            tokio::spawn(async move { server.wait("s3cret", Duration::from_secs(5)).await });

        assert!(get(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        assert!(get(port, "/callback?state=wrong&code=x")
            .await
            .starts_with("HTTP/1.1 400"));
        assert!(
            get(port, "/callback?code=x")
                .await
                .starts_with("HTTP/1.1 400"),
            "missing state"
        );
        let ok = get(
            port,
            "/callback?state=s3cret&openid.mode=id_res&name=a%20b+c",
        )
        .await;
        assert!(ok.starts_with("HTTP/1.1 200") && ok.contains("Angemeldet"));

        let callback = waiting.await.unwrap().unwrap();
        assert_eq!(callback.params["openid.mode"], "id_res");
        assert_eq!(callback.params["name"], "a b c");
        assert_eq!(
            callback.url,
            format!(
                "http://127.0.0.1:{port}/callback?state=s3cret&openid.mode=id_res&name=a%20b+c"
            )
        );
    }

    #[tokio::test]
    async fn times_out_and_listens_on_loopback_only() {
        let server = LoopbackServer::bind().await.unwrap();
        assert!(server.listener.local_addr().unwrap().ip().is_loopback());
        let err = server
            .wait("x", Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::LoginFailed(_)));
    }

    #[tokio::test]
    async fn ignores_garbage_and_oversized_requests() {
        let server = LoopbackServer::bind().await.unwrap();
        let port = server.port;
        let waiting = tokio::spawn(async move { server.wait("ok", Duration::from_secs(5)).await });
        let mut junk = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        junk.write_all(&vec![b'A'; MAX_REQUEST_BYTES + 10])
            .await
            .ok();
        drop(junk);
        assert!(get(port, "/callback?state=ok")
            .await
            .starts_with("HTTP/1.1 200"));
        assert!(waiting.await.unwrap().is_ok());
    }

    #[test]
    fn parses_queries() {
        let q = parse_query("a=1&b=x%2By&c&d=hello+world");
        assert_eq!(q["a"], "1");
        assert_eq!(q["b"], "x+y");
        assert_eq!(q["c"], "");
        assert_eq!(q["d"], "hello world");
    }
}
