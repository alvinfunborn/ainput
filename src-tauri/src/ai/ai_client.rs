//! AI 客户端模块，负责与 AI 服务通信，获取候选词

use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::io::{BufRead, BufReader};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{thread, time::Duration};

#[cfg(target_os = "windows")]
use log::error;
use log::info;
use serde_json::json;

use crate::ai::privacy;
use crate::config::{self, ai_client::AiProvider};
use crate::context::Context;
use crate::db::ai_token_usage::increment_used_token;
use crate::db::conn::establish_connection;

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

    pub async fn stream_request_ai<F>(
        &self,
        context: Context,
        on_token: F,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        let config = config::get_config().unwrap().ai_client;
        match config.provider {
            AiProvider::API => {
                if config.api_key.is_empty() {
                    self.stream_request_mock(context, on_token, cancel_token)
                        .await?;
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
    pub async fn stream_request_mock<F>(
        &self,
        context: Context,
        mut on_token: F,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
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

        info!(
            "[AiClient::stream_request_mock] mock response: {}",
            mock_response
        );

        let chars: Vec<String> = mock_response.chars().map(|c| c.to_string()).collect();
        std::thread::spawn(move || {
            for c in chars {
                if cancel_token.load(Ordering::SeqCst) {
                    break;
                }
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

    #[cfg(target_os = "macos")]
    async fn stream_cmd<F>(
        &self,
        context: Context,
        on_token: F,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        let command = config::get_config().unwrap().ai_client.cmd;
        let prompt = self.prompt_text(context);
        tokio::task::spawn_blocking(move || {
            super::unix_cmd::run(command, prompt, on_token, cancel_token)
        })
        .await?
    }

    #[cfg(target_os = "windows")]
    async fn stream_cmd<F>(
        &self,
        context: Context,
        mut on_token: F,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
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

        info!(
            "[AiClient::stream_cmd] executing: {}, use_stdin: {}",
            final_cmd, use_stdin
        );

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
                        error!(
                            "[AiClient::stream_cmd] error reading line from stdout: {:?}",
                            e
                        );
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
                    info!(
                        "[AiClient::stream_cmd] killing child process {} due to cancellation",
                        child_id
                    );
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

        info!(
            "[AiClient::stream_cmd] stream finished with status: {:?}",
            exit_status
        );

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
                    format!(
                        "CMD error (exit {}): {}",
                        exit_code,
                        stderr_content.chars().take(200).collect::<String>()
                    )
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

    async fn stream_api<F>(
        &self,
        context: Context,
        on_token: F,
        cancel_token: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        if cancel_token.load(Ordering::SeqCst) {
            return Ok(());
        }
        let config = config::get_config().unwrap().ai_client;
        let prompt = self.prompt_text(context);
        let anonymized = privacy::anonymize(&prompt);
        let prompt_token_count = prompt.chars().count() as i64;
        let apikey = config.api_key.clone();
        let mut conn = establish_connection();
        let mut counted_prompt = false;
        let mut on_token = on_token;
        let mut processor = StreamingDeanonymizer::new(anonymized.mapping, move |token: String| {
            if !counted_prompt {
                increment_used_token(&mut conn, &apikey, prompt_token_count);
                counted_prompt = true;
            }
            increment_used_token(&mut conn, &apikey, token.chars().count() as i64);
            on_token(token);
        });
        super::api_transport::stream(
            &config,
            &anonymized.text,
            |token| processor.process(&token),
            &cancel_token,
        )
        .await?;
        if !cancel_token.load(Ordering::SeqCst) {
            processor.flush();
        }
        Ok(())
    }
}
