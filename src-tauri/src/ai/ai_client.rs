//! AI 客户端模块，负责与 AI 服务通信，获取候选词

use std::{thread, time::Duration};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::io::{BufReader, BufRead};

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
        let config = config::get_config().unwrap().ai_client;
        let prompt = self.prompt_text(context);
        let command_str = config.cmd;

        let mut command_parts = command_str.split_whitespace();
        let executable = match command_parts.next() {
            Some(e) => e,
            None => return Err("Invalid cmd in config: missing executable".into()),
        };
        
        let mut cmd = Command::new(executable);
        cmd.args(command_parts);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        info!("[AiClient::stream_cmd] command: {:?}, prompt: {}", cmd, prompt);

        let mut child = cmd.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(prompt.as_bytes())?;
        }

        let stdout = child.stdout.take().ok_or_else(|| "Failed to open stdout".to_string())?;
        let reader = BufReader::new(stdout);

        let handle = thread::spawn(move || {
            for line in reader.lines() {
                if cancel_token.load(Ordering::SeqCst) {
                    info!("[AiClient::stream_cmd] stream cancelled by token");
                    break;
                }
                match line {
                    Ok(line) => {
                        on_token(line + "\n");
                    }
                    Err(e) => {
                        error!("[AiClient::stream_cmd] error reading line from stdout: {:?}", e);
                        break;
                    }
                }
            }
        });

        handle.join().unwrap();
        child.wait()?;
        info!("[AiClient::stream_cmd] stream finished");
        Ok(())
    }

    async fn stream_api<F>(&self, context: Context, mut on_token: F, cancel_token: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: FnMut(String) + Send + 'static,
    {
        info!("[AiClient::stream_api] starting AI stream request");
        let config = config::get_config().unwrap().ai_client;
        let prompt = self.prompt_text(context);

        let anonymized_data = privacy::anonymize(&prompt);
        let anonymized_prompt = anonymized_data.text;
        info!("[AiClient::stream_api] anonymized_prompt: {:?}", anonymized_prompt);
        let mapping = anonymized_data.mapping;
        debug!("[AiClient::stream_api] mapping: {:?}", mapping);

        enum Processor {
            Passthrough(Box<dyn FnMut(String) + Send>),
            Deanonymizing(StreamingDeanonymizer<Box<dyn FnMut(String) + Send>>),
        }

        let apikey = config.api_key.clone();
        let mut conn = establish_connection();
        let prompt_token_count = prompt.chars().count() as i64;
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

        let client = Client::builder().no_proxy().build().unwrap();
        let req = client
            .post(config.api_url)
            .bearer_auth(config.api_key)
            .json(&serde_json::json!({
                "model": config.api_model,
                "messages": [
                    { "role": "user", "content": anonymized_prompt }
                ],
                "stream": true
            }))
            .send()
            .await?;

        info!("[AiClient::stream_api] request sent, waiting for stream response");

        let mut stream = req.bytes_stream();
        while let Some(item) = stream.next().await {
            if cancel_token.load(Ordering::SeqCst) {
                info!("[AiClient::stream_api] stream cancelled by token");
                break;
            }
            match item {
                Ok(chunk) => {
                    for line in chunk.split(|&b| b == b'\n') {
                        if line.starts_with(b"data: ") {
                            let json_str = &line[6..];
                            if json_str == b"[DONE]" { continue; }
                            if let Ok(val) = serde_json::from_slice::<Value>(json_str) {
                                if let Some(token) = val["choices"][0]["delta"]["content"].as_str() {
                                    match &mut processor {
                                        Processor::Passthrough(f) => f(token.to_string()),
                                        Processor::Deanonymizing(d) => d.process(token),
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("[AiClient::stream_api] error: {:?}", e);
                    return Err(Box::new(e));
                }
            }
        }

        if let Processor::Deanonymizing(mut d) = processor {
            d.flush();
        }

        info!("[AiClient::stream_api] stream finished");
        Ok(())
    }
}


