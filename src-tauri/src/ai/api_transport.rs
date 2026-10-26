//! Shared API transport for suggestions and the settings connection test.
use crate::config::AiClientConfig;
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use reqwest::{Client, Proxy};
use serde::Serialize;
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

type Error = Box<dyn std::error::Error + Send + Sync>;
static CLIENT: Lazy<Mutex<Option<CachedClient>>> = Lazy::new(|| Mutex::new(None));

struct CachedClient {
    proxy: String,
    rustls: bool,
    checked: Instant,
    snapshot: Option<Vec<String>>,
    client: Client,
}

// Keep credentials and proxy details in memory only. A configuration check must
// not discard healthy pooled connections when the route settings are unchanged.
fn proxy_snapshot() -> Option<Vec<String>> {
    #[cfg(target_os = "windows")]
    return None; // Retain periodic refresh until registry change detection exists.
    #[cfg(not(target_os = "windows"))]
    {
        let mut values: Vec<String> = [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
            "REQUEST_METHOD",
        ]
        .iter()
        .map(|name| format!("{name}={:?}", std::env::var_os(name)))
        .collect();
        #[cfg(target_os = "macos")]
        values.extend(macos_proxy_snapshot()?);
        Some(values)
    }
}

#[cfg(target_os = "macos")]
fn macos_proxy_snapshot() -> Option<Vec<String>> {
    use core_foundation::{
        base::{CFType, TCFType},
        dictionary::{CFDictionary, CFDictionaryRef},
        number::CFNumber,
        string::CFString,
    };
    #[link(name = "SystemConfiguration", kind = "framework")]
    extern "C" {
        fn SCDynamicStoreCopyProxies(store: *const std::ffi::c_void) -> CFDictionaryRef;
    }
    let raw = unsafe { SCDynamicStoreCopyProxies(std::ptr::null()) };
    if raw.is_null() {
        return None;
    }
    let proxies: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(raw) };
    // These are the settings reqwest's macOS resolver actually consumes.
    Some(
        [
            "HTTPEnable",
            "HTTPProxy",
            "HTTPPort",
            "HTTPSEnable",
            "HTTPSProxy",
            "HTTPSPort",
        ]
        .iter()
        .map(|key| {
            let value = proxies.find(&CFString::new(key)).and_then(|value| {
                value
                    .downcast::<CFString>()
                    .map(|v| v.to_string())
                    .or_else(|| {
                        value
                            .downcast::<CFNumber>()
                            .and_then(|v| v.to_i64())
                            .map(|v| v.to_string())
                    })
            });
            format!("{key}={value:?}")
        })
        .collect(),
    )
}

// Google may wrap an HTTP error in an array when stream=true, even before SSE starts.
fn api_error(body: &str, status: u16, key: &str) -> String {
    let value: Value = serde_json::from_str(body).unwrap_or_default();
    let envelope = value
        .as_array()
        .and_then(|items| items.iter().find(|item| item.get("error").is_some()))
        .unwrap_or(&value);
    let error = envelope.get("error").unwrap_or(envelope);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or("Request rejected; the server returned no readable error message");
    let key = key.trim();
    let message = if key.is_empty() {
        message.to_owned()
    } else {
        message.replace(key, "[redacted]")
    };
    let code = error.get("status").and_then(Value::as_str).unwrap_or("");
    let reason = if code.is_empty() {
        String::new()
    } else {
        format!(" {code}")
    };
    format!(
        "API error ({status}{reason}): {}",
        message.chars().take(600).collect::<String>()
    )
}

fn build_client(proxy: &str, rustls: bool) -> Result<Client, Error> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(60));
    if rustls {
        // Enabling the Cargo feature alone still selects native TLS by default.
        // Explicit Rustls selection also enables HTTP/2 negotiation for Gemini.
        builder = builder.use_rustls_tls();
    }
    // Reqwest reads proxy environment variables, falling back to macOS / Windows
    // settings. Do not call no_proxy(): Finder launches have no shell environment.
    if !proxy.is_empty() {
        let url = reqwest::Url::parse(proxy).map_err(|_| "Invalid API proxy URL")?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err("API proxy must be an http:// or https:// URL".into());
        }
        builder = builder.proxy(Proxy::all(proxy)?);
    }
    Ok(builder.build()?)
}

fn client(proxy: &str, api_url: &str) -> Result<Client, Error> {
    // Keep the system certificate trust behavior of custom/enterprise endpoints.
    let rustls = reqwest::Url::parse(api_url)
        .map(|url| {
            url.scheme() == "https" && url.host_str() == Some("generativelanguage.googleapis.com")
        })
        .unwrap_or(false);
    cached_client(
        &mut CLIENT.lock().unwrap(),
        proxy,
        rustls,
        Instant::now(),
        proxy_snapshot,
    )
}

fn cached_client(
    cache: &mut Option<CachedClient>,
    proxy: &str,
    rustls: bool,
    now: Instant,
    snapshot: impl FnOnce() -> Option<Vec<String>>,
) -> Result<Client, Error> {
    let proxy = proxy.trim();
    if let Some(cached) = cache.as_ref() {
        if cached.proxy == proxy
            && cached.rustls == rustls
            && (!proxy.is_empty() || now.duration_since(cached.checked) < Duration::from_secs(30))
        {
            return Ok(cached.client.clone());
        }
    }
    let current = if proxy.is_empty() { snapshot() } else { None };
    if let Some(cached) = cache.as_mut() {
        if cached.proxy == proxy
            && cached.rustls == rustls
            && current.is_some()
            && cached.snapshot == current
        {
            cached.checked = now;
            return Ok(cached.client.clone());
        }
    }
    let client = build_client(proxy, rustls)?;
    *cache = Some(CachedClient {
        proxy: proxy.into(),
        rustls,
        checked: now,
        snapshot: current,
        client: client.clone(),
    });
    Ok(client)
}

#[derive(Default)]
struct SseDecoder {
    pending: Vec<u8>,
    data: Vec<u8>,
    done: bool,
}

impl SseDecoder {
    fn event(&mut self, output: &mut Vec<String>) -> Result<(), Error> {
        if self.data.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.data);
        if data == b"[DONE]" {
            self.done = true;
            return Ok(());
        }
        let value: Value =
            serde_json::from_slice(&data).map_err(|_| "Invalid JSON in API stream")?;
        if value.get("error").is_some() {
            return Err(api_error(&String::from_utf8_lossy(&data), 200, "").into());
        }
        if let Some(text) = value["choices"][0]["delta"]["content"].as_str() {
            if !text.is_empty() {
                output.push(text.into());
            }
        }
        Ok(())
    }

    fn line(&mut self, line: &[u8], output: &mut Vec<String>) -> Result<(), Error> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            return self.event(output);
        }
        if let Some(data) = line.strip_prefix(b"data:") {
            if !self.data.is_empty() {
                self.data.push(b'\n');
            }
            self.data
                .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
        }
        Ok(())
    }

    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>, Error> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() + self.data.len() > 1_048_576 {
            return Err("API stream event is too large".into());
        }
        let mut output = Vec::new();
        while let Some(end) = self.pending.iter().position(|&byte| byte == b'\n') {
            let line: Vec<_> = self.pending.drain(..=end).collect();
            self.line(&line[..line.len() - 1], &mut output)?;
            if self.done {
                break;
            }
        }
        Ok(output)
    }

    fn finish(&mut self) -> Result<Vec<String>, Error> {
        let mut output = Vec::new();
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() {
            self.line(&pending, &mut output)?;
        }
        self.event(&mut output)?;
        Ok(output)
    }
}

#[derive(Default, Serialize)]
pub struct StreamTiming {
    pub first_token_ms: Option<u64>,
    pub total_ms: u64,
    pub output_chars: usize,
}

async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

pub async fn stream<F>(
    config: &AiClientConfig,
    prompt: &str,
    mut on_token: F,
    cancel: &AtomicBool,
) -> Result<StreamTiming, Error>
where
    F: FnMut(String) + Send,
{
    let started = Instant::now();
    let mut timing = StreamTiming::default();
    if cancel.load(Ordering::SeqCst) {
        return Ok(timing);
    }
    let request = client(&config.api_proxy, &config.api_url)?
        .post(&config.api_url)
        .bearer_auth(config.api_key.trim())
        .json(&serde_json::json!({
            "model": config.api_model,
            "messages": [{"role": "user", "content": prompt}],
            "stream": true
        }));
    log::info!(
        "[ai_api] start proxy={}",
        if config.api_proxy.trim().is_empty() {
            "automatic"
        } else {
            "custom"
        }
    );
    let response = tokio::select! {
        result = request.send() => result.map_err(|e| {
            log::warn!("[ai_api] request_failed elapsed_ms={} timeout={} connect={}", started.elapsed().as_millis(), e.is_timeout(), e.is_connect());
            e.without_url()
        })?,
        _ = cancelled(cancel) => return Ok(timing),
    };
    let status = response.status();
    log::info!(
        "[ai_api] headers status={} protocol={:?} elapsed_ms={}",
        status.as_u16(),
        response.version(),
        started.elapsed().as_millis()
    );
    if !status.is_success() {
        let body = tokio::select! {
            body = response.text() => body.unwrap_or_default(),
            _ = cancelled(cancel) => return Ok(timing),
        };
        return Err(api_error(&body, status.as_u16(), &config.api_key).into());
    }
    let mut body = response.bytes_stream();
    let mut decoder = SseDecoder::default();
    let mut emit = |tokens: Vec<String>| {
        for token in tokens {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            if timing.first_token_ms.is_none() {
                timing.first_token_ms = Some(started.elapsed().as_millis() as u64);
                log::info!("[ai_api] first_token_ms={}", timing.first_token_ms.unwrap());
            }
            timing.output_chars += token.chars().count();
            on_token(token);
        }
    };
    loop {
        let chunk = tokio::select! {
            next = body.next() => next,
            _ = cancelled(cancel) => return Ok(StreamTiming { total_ms: started.elapsed().as_millis() as u64, ..timing }),
        };
        match chunk {
            Some(chunk) => emit(
                decoder
                    .push(&chunk.map_err(|e| e.without_url())?)
                    .map_err(|e| redact_stream_error(e, &config.api_key))?,
            ),
            None => {
                emit(
                    decoder
                        .finish()
                        .map_err(|e| redact_stream_error(e, &config.api_key))?,
                );
                break;
            }
        }
        if decoder.done {
            break;
        }
    }
    timing.total_ms = started.elapsed().as_millis() as u64;
    log::info!(
        "[ai_api] finished total_ms={} output_chars={}",
        timing.total_ms,
        timing.output_chars
    );
    Ok(timing)
}

fn redact_stream_error(error: Error, key: &str) -> Error {
    let message = error.to_string();
    if key.trim().is_empty() {
        message.into()
    } else {
        message.replace(key.trim(), "[redacted]").into()
    }
}

#[tauri::command]
pub async fn test_api_connection(config: AiClientConfig) -> Result<StreamTiming, String> {
    if config.api_key.trim().is_empty() {
        return Err("Enter your Google/API key first".into());
    }
    stream(
        &config,
        "Complete the text 'hello wo'. Return only the missing suffix, no explanation.",
        |_| {},
        &AtomicBool::new(false),
    )
    .await
    .map_err(|e| e.to_string())
    .and_then(|timing| {
        if timing.output_chars == 0 {
            Err("API returned no completion text".into())
        } else {
            Ok(timing)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn google_streaming_http_errors_preserve_reason_and_redact_key() {
        let body = r#"[{"error":{"code":400,"message":"User location is not supported for the API use.","status":"FAILED_PRECONDITION"}}]"#;
        let error = api_error(body, 400, "");
        assert!(error.contains("User location is not supported"));
        assert!(error.contains("FAILED_PRECONDITION"));
        let error = api_error(
            r#"{"error":{"message":"Invalid key secret-key"}}"#,
            401,
            " secret-key ",
        );
        assert!(!error.contains("secret-key"));
        assert!(error.contains("[redacted]"));
        assert!(
            !api_error("<html>private proxy body</html>", 502, "").contains("private proxy body")
        );
    }
    #[test]
    fn sse_preserves_every_split_including_utf8_and_crlf() {
        let input = "data: {\"choices\":[{\"delta\":{\"content\":\"你好😀\"}}]}\r\n\r\ndata:{\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\ndata: [DONE]\n\n".as_bytes();
        for split in 0..=input.len() {
            let mut decoder = SseDecoder::default();
            let mut output = decoder.push(&input[..split]).unwrap();
            output.extend(decoder.push(&input[split..]).unwrap());
            assert_eq!(output.concat(), "你好😀 world");
            assert!(decoder.done);
        }
        let mut decoder = SseDecoder::default();
        let output: Vec<_> = input
            .iter()
            .flat_map(|byte| decoder.push(&[*byte]).unwrap())
            .collect();
        assert_eq!(output.concat(), "你好😀 world");
    }

    #[test]
    fn sse_reports_errors_instead_of_silently_dropping_events() {
        assert!(SseDecoder::default().push(b"data: {invalid}\n\n").is_err());
        assert!(SseDecoder::default()
            .push(b"data: {\"error\":{\"message\":\"quota\"}}\n\n")
            .unwrap_err()
            .to_string()
            .contains("quota"));
        let mut decoder = SseDecoder::default();
        decoder
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}")
            .unwrap();
        assert_eq!(decoder.finish().unwrap(), vec!["ok"]);
    }

    #[tokio::test]
    async fn explicit_proxy_routes_request_without_resolving_target() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut data = [0; 4096];
            let count = socket.read(&mut data).unwrap();
            assert!(String::from_utf8_lossy(&data[..count])
                .starts_with("GET http://ainput-proxy-test.invalid/check HTTP/1.1"));
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let result = build_client(&format!("http://{addr}"), false)
            .unwrap()
            .get("http://ainput-proxy-test.invalid/check")
            .send()
            .await
            .unwrap();
        assert_eq!(result.text().await.unwrap(), "ok");
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn stable_proxy_recheck_preserves_a_live_connection() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // Both requests must use this single accepted TCP connection, including
        // one after the proxy-settings refresh deadline has elapsed.
        let worker = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(socket);
            for _ in 0..2 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert!(line.starts_with("GET http://ainput-pool-test.invalid/check HTTP/1.1"));
                loop {
                    line.clear();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                }
                reader
                    .get_mut()
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .unwrap();
            }
        });
        let now = Instant::now();
        let snapshot = || Some(vec!["unchanged-proxy".into()]);
        let mut cache = Some(CachedClient {
            proxy: String::new(),
            rustls: false,
            checked: now,
            snapshot: snapshot(),
            client: build_client(&format!("http://{addr}"), false).unwrap(),
        });
        for elapsed in [0, 31] {
            let client = cached_client(
                &mut cache,
                "",
                false,
                now + Duration::from_secs(elapsed),
                snapshot,
            )
            .unwrap();
            assert_eq!(
                client
                    .get("http://ainput-pool-test.invalid/check")
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap(),
                "ok"
            );
        }
        worker.join().unwrap();
        cached_client(&mut cache, "", false, now + Duration::from_secs(62), || {
            Some(vec!["changed-proxy".into()])
        })
        .unwrap();
        assert_eq!(cache.unwrap().snapshot, Some(vec!["changed-proxy".into()]));
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_stalled_response_body() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = [0; 4096];
            socket.read(&mut request).unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap();
            std::thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::SeqCst);
            let _ = wait.recv_timeout(Duration::from_secs(3));
        });
        let config: AiClientConfig = serde_json::from_value(serde_json::json!({
            "provider": "API", "api_key": "test-only", "api_url": "http://ainput-proxy-test.invalid/chat",
            "api_model": "test", "api_proxy": format!("http://{addr}"), "cmd": "", "prompt": ""
        })).unwrap();
        let started = Instant::now();
        let result = stream(
            &config,
            "synthetic test",
            |_| panic!("No content expected"),
            &cancel,
        )
        .await;
        let _ = release.send(());
        worker.join().unwrap();
        assert!(result.is_ok());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    #[ignore = "manual network probe without credentials; run with proxy environment cleared to exercise macOS settings"]
    async fn system_proxy_reaches_google_without_api_key() {
        let response = build_client("", false)
            .unwrap()
            .get("https://generativelanguage.googleapis.com/v1beta/models")
            .send()
            .await
            .unwrap();
        assert!(response.status().is_client_error());
    }
}
