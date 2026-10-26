//! macOS CLI provider: isolated process groups and data-only prompt expansion.
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
type Error = Box<dyn std::error::Error + Send + Sync>;

fn replace_argument(command: &str, placeholder: &str, variable: &str) -> String {
    let expansion = format!("\"${{{variable}}}\"");
    command
        .replace(&format!("\"{placeholder}\""), &expansion)
        .replace(&format!("'{placeholder}'"), &expansion)
        .replace(placeholder, &expansion)
}

pub fn run<F>(
    template: String,
    prompt: String,
    mut on_token: F,
    cancel: Arc<AtomicBool>,
) -> Result<(), Error>
where
    F: FnMut(String) + Send + 'static,
{
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    let file_mode = template.contains("{{prompt_file}}");
    let argument_mode = template.contains("{{prompt}}");
    let mut file = if file_mode {
        Some(tempfile::NamedTempFile::new()?)
    } else {
        None
    };
    if let Some(file) = file.as_mut() {
        file.write_all(prompt.as_bytes())?;
    }
    let command = replace_argument(
        &replace_argument(&template, "{{prompt_file}}", "AINPUT_PROMPT_FILE"),
        "{{prompt}}",
        "AINPUT_PROMPT",
    );
    let mut builder = Command::new("/bin/sh");
    builder
        .args(["-c", &command])
        .env("AINPUT_PROMPT", &prompt)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(file) = file.as_ref() {
        builder.env("AINPUT_PROMPT_FILE", file.path());
    }
    let mut child = builder.spawn()?;
    let mut stdin = child.stdin.take().ok_or("CLI stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("CLI stdout unavailable")?;
    let mut stderr = child.stderr.take().ok_or("CLI stderr unavailable")?;
    // Drain stderr concurrently so a verbose CLI cannot deadlock its stdout.
    let errors = thread::spawn(move || {
        let mut saved = Vec::new();
        let mut buffer = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let keep = n.min(64 * 1024 - saved.len());
            saved.extend_from_slice(&buffer[..keep]);
        }
        String::from_utf8_lossy(&saved).to_string()
    });
    let cancelled = cancel.clone();
    let monitor = thread::spawn(move || -> std::io::Result<std::process::ExitStatus> {
        loop {
            // Check/reap before considering cancellation, avoiding a late signal
            // after a completed child PID has been reused.
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if cancelled.load(Ordering::SeqCst) {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                return child.wait();
            }
            thread::sleep(Duration::from_millis(25));
        }
    });
    let input_result = if !file_mode && !argument_mode {
        stdin.write_all(prompt.as_bytes())
    } else {
        Ok(())
    };
    drop(stdin);
    if input_result.is_err() {
        cancel.store(true, Ordering::SeqCst);
    }
    let mut got_output = false;
    for line in BufReader::new(stdout).lines() {
        match line {
            Ok(line) if !cancel.load(Ordering::SeqCst) => {
                got_output = true;
                on_token(line + "\n");
            }
            Ok(_) => {}
            Err(_) => {
                cancel.store(true, Ordering::SeqCst);
                break;
            }
        }
    }
    let status = monitor.join().map_err(|_| "CLI monitor failed")??;
    let stderr = errors.join().unwrap_or_default();
    input_result?;
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }
    if !status.success() {
        return Err(format!(
            "CLI exited with {status}: {}",
            stderr.chars().take(200).collect::<String>()
        )
        .into());
    }
    if !got_output {
        return Err("CLI returned no output".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[test]
    fn prompt_is_data_for_stdin_file_and_quoted_argument_modes() {
        let prompt = "你好 😀 ' \" $HOME $(printf injected) `printf injected`";
        for command in [
            "cat",
            "cat <{{prompt_file}}",
            "printf '%s\\n' {{prompt}}",
            "printf '%s\\n' \"{{prompt}}\"",
        ] {
            let output = Arc::new(Mutex::new(String::new()));
            let capture = output.clone();
            run(
                command.into(),
                prompt.into(),
                move |s| capture.lock().unwrap().push_str(&s),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            assert_eq!(output.lock().unwrap().trim_end(), prompt);
        }
    }
    #[test]
    fn cancellation_terminates_a_silent_child_process_group() {
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let start = std::time::Instant::now();
        let trigger = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            signal.store(true, Ordering::SeqCst);
        });
        run("sleep 30".into(), String::new(), |_| {}, cancel).unwrap();
        trigger.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
