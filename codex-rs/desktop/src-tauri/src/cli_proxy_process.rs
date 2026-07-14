use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Result;
use anyhow::bail;
use tokio::net::TcpStream;

pub(super) const START_TIMEOUT: Duration = Duration::from_secs(12);
pub(super) const START_POLL_INTERVAL: Duration = Duration::from_millis(150);

pub(super) async fn proxy_port_is_open() -> bool {
    tokio::time::timeout(
        Duration::from_millis(350),
        TcpStream::connect(("127.0.0.1", 8317)),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

pub(super) fn proxy_command(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    if let Some(parent) = binary.parent() {
        command.current_dir(parent);
    }
    if let Some(config_path) = proxy_config_path(binary) {
        command.arg("-config").arg(config_path);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

pub(super) fn terminate_child(mut child: Child) -> std::io::Result<()> {
    if child.try_wait()?.is_none() {
        child.kill()?;
    }
    let _ = child.wait()?;
    Ok(())
}

fn proxy_config_path(binary: &Path) -> Option<PathBuf> {
    std::env::var_os("CLIPROXYAPI_CONFIG_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            binary
                .parent()
                .map(|parent| parent.join("config.yaml"))
                .filter(|path| path.is_file())
        })
}

pub(super) fn resolve_binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CLIPROXYAPI_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Ok(path);
    }

    for name in binary_names() {
        if let Ok(path) = which::which(name) {
            return Ok(path);
        }
    }

    for path in standard_binary_candidates() {
        if path.is_file() {
            return Ok(path);
        }
    }

    bail!("CLIProxyAPI was not found. Set CLIPROXYAPI_PATH or place the binary beside the app")
}

fn standard_binary_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(parent) = executable.parent()
    {
        roots.push(parent.to_path_buf());
        roots.push(parent.join("binaries"));
        roots.push(parent.join("resources"));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".local").join("bin"));
        roots.push(home.join(".cli-proxy-api"));
    }
    #[cfg(windows)]
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local_app_data).join("CLIProxyAPI"));
    }
    #[cfg(not(windows))]
    roots.push(PathBuf::from("/usr/local/bin"));

    roots
        .into_iter()
        .flat_map(|root| binary_names().iter().map(move |name| root.join(name)))
        .collect()
}

#[cfg(windows)]
fn binary_names() -> &'static [&'static str] {
    &["cli-proxy-api.exe", "cliproxyapi.exe", "CLIProxyAPI.exe"]
}

#[cfg(not(windows))]
fn binary_names() -> &'static [&'static str] {
    &["cli-proxy-api", "cliproxyapi", "CLIProxyAPI"]
}
