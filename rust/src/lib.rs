#![allow(clippy::async_yields_async)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use pumpkin::plugin::Context;
use pumpkin_api_macros::{plugin_impl, plugin_method};
const READY_LINE: &str = "PATCHQUILT_READY";

struct QuiltProcess {
    child: Child,
    stdin: ChildStdin,
}

fn setup_directories() -> Result<(PathBuf, PathBuf), String> {
    let server_root = std::env::current_dir()
        .map_err(|error| format!("failed to resolve server root: {error}"))?;
    let base = server_root.join("patchquilt");
    let mods = base.join("mods");
    let runtime = base.join("runtime");
    std::fs::create_dir_all(&mods)
        .map_err(|error| format!("failed to create {}: {error}", mods.display()))?;
    std::fs::create_dir_all(runtime.join("lib"))
        .map_err(|error| format!("failed to create {}: {error}", runtime.display()))?;
    Ok((base, runtime))
}

fn java_executable() -> PathBuf {
    std::env::var_os("PATCHQUILT_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from)
}

fn classpath(runtime: &Path) -> PathBuf {
    runtime.join("lib").join("*")
}

fn start_runtime(base: &Path, runtime: &Path) -> Result<QuiltProcess, String> {
    let mut command = Command::new(java_executable());
    command
        .arg("-cp")
        .arg(classpath(runtime))
        .arg("org.patchquilt.host.PatchQuiltLauncher")
        .arg("--gameDir")
        .arg(base)
        .current_dir(base)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start PatchQuilt Java runtime: {error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "PatchQuilt Java runtime has no stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "PatchQuilt Java runtime has no stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "PatchQuilt Java runtime has no stderr".to_string())?;

    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tracing::warn!(target: "patchquilt_java", "{line}");
        }
    });

    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut ready_sender = Some(ready_sender);
        for line in BufReader::new(stdout).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    if let Some(sender) = ready_sender.take() {
                        let _ =
                            sender.send(Err(format!("failed to read PatchQuilt output: {error}")));
                    }
                    return;
                }
            };
            if line == READY_LINE {
                if let Some(sender) = ready_sender.take() {
                    let _ = sender.send(Ok(()));
                }
            } else {
                tracing::info!(target: "patchquilt_java", "{line}");
            }
        }
        if let Some(sender) = ready_sender {
            let _ = sender.send(Err(
                "PatchQuilt Java runtime exited before becoming ready".to_string()
            ));
        }
    });

    let readiness = ready_receiver
        .recv_timeout(Duration::from_mins(1))
        .map_err(|_| "PatchQuilt Java runtime did not become ready within 60 seconds".to_string())
        .and_then(|result| result);
    if let Err(error) = readiness {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    Ok(QuiltProcess { child, stdin })
}

fn on_load_inner(plugin: &PatchQuiltPlugin, context: &Context) -> Result<(), String> {
    context.init_log();
    let (base, runtime) = setup_directories()?;
    let process = start_runtime(&base, &runtime)?;
    *plugin
        .process
        .lock()
        .map_err(|_| "PatchQuilt process lock is poisoned".to_string())? = Some(process);
    tracing::info!("PatchQuilt runtime is ready");
    Ok(())
}

fn on_unload_inner(plugin: &PatchQuiltPlugin) -> Result<(), String> {
    let Some(mut process) = plugin
        .process
        .lock()
        .map_err(|_| "PatchQuilt process lock is poisoned".to_string())?
        .take()
    else {
        return Ok(());
    };
    process
        .stdin
        .write_all(b"STOP\n")
        .map_err(|error| format!("failed to stop PatchQuilt runtime: {error}"))?;
    process
        .stdin
        .flush()
        .map_err(|error| format!("failed to flush PatchQuilt stop command: {error}"))?;
    for _ in 0..100 {
        if process
            .child
            .try_wait()
            .map_err(|error| format!("failed to inspect PatchQuilt runtime: {error}"))?
            .is_some()
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    process
        .child
        .kill()
        .map_err(|error| format!("failed to terminate PatchQuilt runtime: {error}"))?;
    process
        .child
        .wait()
        .map_err(|error| format!("failed to reap PatchQuilt runtime: {error}"))?;
    Ok(())
}

#[plugin_method]
async fn on_load(&self, context: Arc<Context>) -> Result<(), String> {
    on_load_inner(self, &context)
}

#[plugin_method]
async fn on_unload(&self, _context: Arc<Context>) -> Result<(), String> {
    on_unload_inner(self)
}

#[plugin_impl]
pub struct PatchQuiltPlugin {
    process: Mutex<Option<QuiltProcess>>,
}

impl PatchQuiltPlugin {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            process: Mutex::new(None),
        }
    }
}

impl Default for PatchQuiltPlugin {
    fn default() -> Self {
        Self::new()
    }
}
