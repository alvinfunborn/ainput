//! AI 客户端模块，负责与 AI 服务通信，获取候选词

use std::{thread, time::Duration};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::io::{BufReader, BufRead};
use std::os::windows::process::CommandExt;

use log::{debug, error, info};
use serde_json::json;
use reqwest::Client;
use serde_json::Value;
use futures_util::StreamExt;

use crate::context::Context;
use crate::ai::privacy;
use crate::db::conn::establish_connection;
use crate::db::ai_token_usage::increment_used_token;
use crate::config::{self, ai_client::AiProvider};

struct StreamingDeanonymizer<F>
where
    F: FnMut(String) + Send + 'static,
{
    buffer: String,
    mapping: HashMap<String, String>,
    on_token: F,
    max_placeholder_len: usize,
}

impl<F> StreamingDeanonymizer<F>
where
    F: FnMut(String) + Send + 'static,
{
    fn new(mapping: HashMap<String, String>, on_token: F) -> Self {
        let max_placeholder_len = mapping.keys().map(String::len).max().unwrap_or(0);
        Self {
            buffer: String::new(),
            mapping,
            on_token,
            max_placeholder_len,
        }
    }

    fn process(&mut self, token: &str) {
        self.buffer.push_str(token);

        'replacing_loop: loop {
            if let Some((placeholder, original_value)) = self
                .mapping
                .iter()
                .find(|(p, _)| self.buffer.starts_with(*p))
            {
                (&mut self.on_token)(original_value.clone());
                self.buffer = self.buffer[placeholder.len()..].to_string();
                continue;
            }

            let safe_end = self.buffer.find('[').unwrap_or_else(|| {
                if self.buffer.len() >= self.max_placeholder_len {
                    self.buffer.len()
                } else {
                    0
                }
            });

            if safe_end > 0 {
                let (safe, rest) = self.buffer.split_at(safe_end);
                (&mut self.on_token)(safe.to_string());
                self.buffer = rest.to_string();
            }

            if self.buffer.starts_with('[') {
                let is_prefix = self.mapping.keys().any(|p| p.starts_with(&self.buffer));
                if !is_prefix {
                     (&mut self.on_token)(self.buffer.chars().next().unwrap().to_string());
                     self.buffer = self.buffer[1..].to_string();
                     continue;
                }
            }

            break 'replacing_loop;
        }
    }

    fn flush(&mut self) {
        if !self.buffer.is_empty() {
            (&mut self.on_token)(self.buffer.clone());
            self.buffer.clear();
        }
    }
}

pub struct AiClient {
    // 未来可扩展：API 地址配置、异步请求、mock/真实切换等
}

impl AiClient {
    pub fn new() -> Self {
        info!("[AiClient::new] creating new AiClient");
        AiClient {}
    }

    pub async fn stream_request_ai<F>(&self, context: Context, on_token: F, cancel_token: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        let config = config::get_config().unwrap().ai_client;
        match config.provider {
            AiProvider::API => {
                if config.api_key.is_empty() {
                    self.stream_request_mock(context, on_token, cancel_token).await?;
                } else {
                    self.stream_api(context, on_token, cancel_token).await?;
                }
            }
            AiProvider::CMD => {
                self.stream_cmd(context, on_token, cancel_token).await?;
            }
        }
        Ok(())
    }

    /// 模拟流式请求，每次回调一个字
    pub async fn stream_request_mock<F>(&self, context: Context, mut on_token: F, cancel_token: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        info!("[AiClient::stream_request_mock] starting mock stream request");

        let config = config::get_config().unwrap().ai_client;
        let apikey = config.api_key.clone();
        let mut conn = establish_connection();
        let prompt = self.prompt_text(context.clone());
        let prompt_token_count = prompt.chars().count() as i64;
        increment_used_token(&mut conn, &apikey, prompt_token_count);

        let history_first = context.history.first();
        let clipboard_first = context.clipboard_history.first();
        let mock_response = if let Some(h) = history_first {
            h.input_content.clone()
        } else if let Some(c) = clipboard_first {
            c.clone()
        } else {
            String::new()
        };

        info!("[AiClient::stream_request_mock] mock response: {}", mock_response);

        let chars: Vec<String> = mock_response.chars().map(|c| c.to_string()).collect();
        std::thread::spawn(move || {
            for c in chars {
                if cancel_token.load(Ordering::SeqCst) { break; }
                increment_used_token(&mut conn, &apikey, c.chars().count() as i64);
                on_token(c.clone());
                thread::sleep(Duration::from_millis(50));
            }
        });
        Ok(())
    }

    fn prompt_text(&self, context: Context) -> String {
        let config = config::get_config().unwrap().ai_client;
        let app_name = context.app.window_app;
        let window_title = context.app.window_title;
        let window_handle = context.app.window_id;
        let input_title = context.app.input_title;
        let input_content = context.app.input_content;
        let input_handle = context.app.input_id;
        let input_history = json!(&context.history).to_string();
        let clipboard_contents = json!(&context.clipboard_history).to_string();
        let mut prompt = config.prompt;
        prompt = prompt.replace("{{app_name}}", &app_name);
        prompt = prompt.replace("{{window_title}}", &window_title);
        prompt = prompt.replace("{{window_handle}}", &window_handle);
        prompt = prompt.replace("{{input_title}}", &input_title);
        prompt = prompt.replace("{{input_handle}}", &input_handle);
        prompt = prompt.replace("{{input_content}}", &input_content);
        prompt = prompt.replace("{{input_history}}", &input_history);
        prompt = prompt.replace("{{clipboard_contents}}", &clipboard_contents);
        prompt
    }

    async fn stream_cmd<F>(&self, context: Context, mut on_token: F, cancel_token: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        info!("[AiClient::stream_cmd] starting CLI stream request");

        // Early cancellation check
        if cancel_token.load(Ordering::SeqCst) {
            info!("[AiClient::stream_cmd] cancelled before starting");
            return Ok(());
        }

        let config = config::get_config().unwrap().ai_client;
        let prompt = self.prompt_text(context);
        let command_str = config.cmd.clone();

        // Determine prompt passing mode based on placeholders:
        // - {{prompt_file}}: replace with temp file path (for file redirect like <{{prompt_file}})
        // - {{prompt}}: replace with prompt content directly (for -p "{{prompt}}")
        // - no placeholder: pass prompt via stdin
        let has_prompt_file = command_str.contains("{{prompt_file}}");
        let has_prompt = command_str.contains("{{prompt}}");
        let use_stdin = !has_prompt_file && !has_prompt;

        let temp_file = if has_prompt_file {
            let temp_dir = std::env::temp_dir();
            let path = temp_dir.join(format!("ainput_prompt_{}.txt", std::process::id()));
            std::fs::write(&path, &prompt)?;
            Some(path)
        } else {
            None
        };

        let final_cmd = if has_prompt_file {
            let temp_file_path = temp_file.as_ref().unwrap().to_string_lossy().to_string();
            command_str.replace("{{prompt_file}}", &temp_file_path)
        } else if has_prompt {
            // Escape quotes in prompt for shell
            let escaped_prompt = prompt.replace("\"", "\\\"");
            command_str.replace("{{prompt}}", &escaped_prompt)
        } else {
            command_str.clone()
        };

        info!("[AiClient::stream_cmd] executing: {}, use_stdin: {}", final_cmd, use_stdin);

        // Use cmd.exe /c on Windows to properly handle .cmd/.bat files and shell operators
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd.exe");
            c.args(["/c", &final_cmd]);
            c
        };
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = Command::new("sh");
            c.args(["-c", &final_cmd]);
            c
        };

        if use_stdin {
            cmd.stdin(Stdio::piped());
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        let cleanup_temp = || {
            if let Some(ref path) = temp_file {
                let _ = std::fs::remove_file(path);
            }
        };

        // Check cancellation before spawning
        if cancel_token.load(Ordering::SeqCst) {
            info!("[AiClient::stream_cmd] cancelled before spawning process");
            cleanup_temp();
            return Ok(());
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                cleanup_temp();
                error!("[AiClient::stream_cmd] failed to spawn command: {:?}", e);
                return Err(format!("CMD error: failed to start: {}", e).into());
            }
        };

        // Get child process ID for potential killing
        let child_id = child.id();

        // Write prompt to stdin if no placeholder was used
        if use_stdin {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                if let Err(e) = stdin.write_all(prompt.as_bytes()) {
                    let _ = child.kill();
                    cleanup_temp();
                    error!("[AiClient::stream_cmd] failed to write to stdin: {:?}", e);
                    return Err(format!("CMD error: failed to write input: {}", e).into());
                }
            }
        }

        let stdout = child.stdout.take().ok_or_else(|| {
            let _ = child.kill();
            cleanup_temp();
            "CMD error: failed to open stdout".to_string()
        })?;
        let stderr = child.stderr.take();
        let reader = BufReader::new(stdout);

        // Track if we received any output and if cancelled
        let received_output = Arc::new(AtomicBool::new(false));
        let received_output_clone = received_output.clone();
        let was_cancelled = Arc::new(AtomicBool::new(false));
        let was_cancelled_clone = was_cancelled.clone();
        let cancel_token_clone = cancel_token.clone();

        let handle = thread::spawn(move || {
            for line in reader.lines() {
                if cancel_token_clone.load(Ordering::SeqCst) {
                    info!("[AiClient::stream_cmd] stream cancelled by token");
                    was_cancelled_clone.store(true, Ordering::SeqCst);
                    break;
                }
                match line {
                    Ok(line) => {
                        received_output_clone.store(true, Ordering::SeqCst);
                        on_token(line + "\n");
                    }
                    Err(e) => {
                        error!("[AiClient::stream_cmd] error reading line from stdout: {:?}", e);
                        break;
                    }
                }
            }
        });

        // Monitor cancel_token and kill child process if cancelled
        let cancel_token_monitor = cancel_token.clone();
        let monitor_handle = thread::spawn(move || {
            loop {
                if cancel_token_monitor.load(Ordering::SeqCst) {
                    info!("[AiClient::stream_cmd] killing child process {} due to cancellation", child_id);
                    #[cfg(windows)]
                    {
                        // On Windows, use taskkill to kill the process tree
                        let _ = Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &child_id.to_string()])
                            .creation_flags(0x08000000) // CREATE_NO_WINDOW
                            .output();
                    }
                    #[cfg(not(windows))]
                    {
                        // On Unix, send SIGTERM to process group
                        unsafe {
                            libc::kill(-(child_id as i32), libc::SIGTERM);
                        }
                    }
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        });

        handle.join().unwrap();

        // Wait for child process (may already be killed)
        let exit_status = child.wait();

        // Stop the monitor thread by ensuring cancel_token is checked
        // The monitor will exit naturally after detecting cancellation or when we're done
        drop(monitor_handle); // Let it run to completion or be cleaned up

        info!("[AiClient::stream_cmd] stream finished with status: {:?}", exit_status);

        // If cancelled, don't report errors
        if was_cancelled.load(Ordering::SeqCst) || cancel_token.load(Ordering::SeqCst) {
            cleanup_temp();
            return Ok(());
        }

        // Check if command failed
        if let Ok(status) = exit_status {
            if !status.success() {
                let exit_code = status.code().unwrap_or(-1);
                let stderr_content = if let Some(mut stderr) = stderr {
                    use std::io::Read;
                    let mut buf = String::new();
                    let _ = stderr.read_to_string(&mut buf);
                    buf.trim().to_string()
                } else {
                    String::new()
                };

                let error_msg = if !stderr_content.is_empty() {
                    format!("CMD error (exit {}): {}", exit_code, stderr_content.chars().take(200).collect::<String>())
                } else {
                    format!("CMD error: command exited with code {}", exit_code)
                };
                error!("[AiClient::stream_cmd] {}", error_msg);
                cleanup_temp();
                return Err(error_msg.into());
            }
        }

        // Check if we received any output
        if !received_output.load(Ordering::SeqCst) {
            cleanup_temp();
            return Err("CMD error: no output received".into());
        }

        cleanup_temp();
        Ok(())
    }

    async fn stream_api<F>(&self, context: Context, on_token: F, cancel_token: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        info!("[AiClient::stream_api] starting AI stream request");

        // Early cancellation check before any processing
        if cancel_token.load(Ordering::SeqCst) {
            info!("[AiClient::stream_api] cancelled before starting");
            return Ok(());
        }

        let config = config::get_config().unwrap().ai_client;
        let prompt = self.prompt_text(context);

        let anonymized_data = privacy::anonymize(&prompt);
        let anonymized_prompt = anonymized_data.text;
        info!("[AiClient::stream_api] anonymized_prompt: {:?}", anonymized_prompt);
        let mapping = anonymized_data.mapping;
        debug!("[AiClient::stream_api] mapping: {:?}", mapping);

        // Check cancellation before sending request to save tokens
        if cancel_token.load(Ordering::SeqCst) {
            info!("[AiClient::stream_api] cancelled before sending request");
            return Ok(());
        }

        enum Processor {
            Passthrough(Box<dyn FnMut(String) + Send>),
            Deanonymizing(StreamingDeanonymizer<Box<dyn FnMut(String) + Send>>),
        }

        let apikey = config.api_key.clone();
        let prompt_token_count = prompt.chars().count() as i64;

        let client = Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap();

        let api_url = config.api_url.clone();
        let request = client
            .post(&api_url)
            .bearer_auth(config.api_key.clone())
            .json(&serde_json::json!({
                "model": config.api_model,
                "messages": [
                    { "role": "user", "content": anonymized_prompt }
                ],
                "stream": true
            }));

        // Use tokio::select! to make the request cancellable
        let response: reqwest::Response = tokio::select! {
            res = request.send() => res?,
            _ = async {
                loop {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if cancel_token.load(Ordering::SeqCst) { break; }
                }
            } => {
                info!("[AiClient::stream_api] request cancelled during send");
                return Ok(());
            }
        };

        // Only count prompt tokens after request is successfully sent
        let mut conn = establish_connection();
        increment_used_token(&mut conn, &apikey, prompt_token_count);

        let mut processor = if mapping.is_empty() {
            let mut f = on_token;
            Processor::Passthrough(Box::new(move |token: String| {
                increment_used_token(&mut conn, &apikey, token.chars().count() as i64);
                f(token)
            }))
        } else {
            let mut f = on_token;
            Processor::Deanonymizing(StreamingDeanonymizer::new(mapping, Box::new(move |token: String| {
                increment_used_token(&mut conn, &apikey, token.chars().count() as i64);
                f(token)
            })))
        };

        let status = response.status();
        info!("[AiClient::stream_api] response status: {}", status);

        // Check HTTP status code
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_else(|_| "unable to read response body".to_string());
            error!("[AiClient::stream_api] API returned error status {}: {}", status, error_body);

            // Try to parse error JSON
            let error_msg = if let Ok(val) = serde_json::from_str::<Value>(&error_body) {
                if let Some(msg) = val["error"]["message"].as_str() {
                    format!("API error ({}): {}", status.as_u16(), msg)
                } else if let Some(msg) = val["message"].as_str() {
                    format!("API error ({}): {}", status.as_u16(), msg)
                } else {
                    format!("API error ({}): {}", status.as_u16(), error_body.chars().take(100).collect::<String>())
                }
            } else {
                format!("API error ({}): {}", status.as_u16(), error_body.chars().take(100).collect::<String>())
            };
            return Err(error_msg.into());
        }

        info!("[AiClient::stream_api] request sent, waiting for stream response");

        let mut stream = response.bytes_stream();
        let mut received_any_data = false;

        while let Some(item) = stream.next().await {
            if cancel_token.load(Ordering::SeqCst) {
                info!("[AiClient::stream_api] stream cancelled by token");
                break;
            }
            match item {
                Ok(chunk) => {
                    received_any_data = true;
                    for line in chunk.split(|&b| b == b'\n') {
                        if line.is_empty() { continue; }

                        // Check for non-SSE error response
                        if !line.starts_with(b"data: ") && line.starts_with(b"{") {
                            if let Ok(val) = serde_json::from_slice::<Value>(line) {
                                if let Some(err_msg) = val["error"]["message"].as_str() {
                                    error!("[AiClient::stream_api] API error in response: {}", err_msg);
                                    return Err(format!("API error: {}", err_msg).into());
                                }
                            }
                        }

                        if line.starts_with(b"data: ") {
                            let json_str = &line[6..];
                            if json_str == b"[DONE]" { continue; }
                            if let Ok(val) = serde_json::from_slice::<Value>(json_str) {
                                // Check for errors in stream
                                if let Some(err_msg) = val["error"]["message"].as_str() {
                                    error!("[AiClient::stream_api] API error in stream: {}", err_msg);
                                    return Err(format!("API stream error: {}", err_msg).into());
                                }
                                if let Some(token) = val["choices"][0]["delta"]["content"].as_str() {
                                    match &mut processor {
                                        Processor::Passthrough(f) => f(token.to_string()),
                                        Processor::Deanonymizing(d) => d.process(token),
                                    }
                                }
                            } else {
                                // JSON parse failed, log it
                                let line_str = String::from_utf8_lossy(json_str);
                                if !line_str.trim().is_empty() {
                                    debug!("[AiClient::stream_api] failed to parse JSON: {}", line_str);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("[AiClient::stream_api] stream error: {:?}", e);
                    return Err(Box::new(e));
                }
            }
        }

        if !received_any_data {
            error!("[AiClient::stream_api] no data received from API");
            return Err("API error: no data received".into());
        }

        if let Processor::Deanonymizing(mut d) = processor {
            d.flush();
        }

        info!("[AiClient::stream_api] stream finished");
        Ok(())
    }
}


