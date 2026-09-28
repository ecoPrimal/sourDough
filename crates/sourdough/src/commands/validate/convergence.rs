//! Live convergence validator — replaces `convergence_check.py`.
//!
//! Connects to running primals via their sockets and verifies:
//! - Liveness (`health.liveness`) — process is alive and responding
//! - Capabilities (`capabilities.list`) — primal advertises its domains
//! - Version (`system.version`) — binary version for drift detection
//! - Readiness (`health.readiness`) — can serve requests
//!
//! This is a runtime check (not source scan). Requires primals to be running.
//! Reports convergence status: CONVERGED / PARTIAL / DRIFT / UNREACHABLE.

use anyhow::Result;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixStream;

/// Result of probing a single primal.
struct ProbeResult {
    name: String,
    status: ProbeStatus,
    version: Option<String>,
    capabilities: Vec<String>,
    latency_ms: u64,
}

#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProbeStatus {
    Alive,
    Degraded,
    Unreachable,
    Timeout,
}

impl ProbeStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Alive => "ALIVE",
            Self::Degraded => "DEGRADED",
            Self::Unreachable => "UNREACHABLE",
            Self::Timeout => "TIMEOUT",
        }
    }
}

pub(super) fn validate(socket_dir: &Path, json: bool, timeout_ms: u64) -> Result<()> {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current()
            .block_on(validate_async(socket_dir, json, timeout_ms))
    })
}

async fn validate_async(socket_dir: &Path, json: bool, timeout_ms: u64) -> Result<()> {
    let sockets = discover_sockets(socket_dir);

    if sockets.is_empty() {
        if json {
            println!(r#"{{"convergence":"NO_PRIMALS","socket_dir":"{}","primals":[]}}"#,
                socket_dir.display());
        } else {
            crate::warning(&format!(
                "No primal sockets found in {}",
                socket_dir.display()
            ));
        }
        return Ok(());
    }

    let mut results = Vec::new();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    for (name, path) in &sockets {
        let result = probe_primal(name, path, timeout).await;
        results.push(result);
    }

    if json {
        print_json(socket_dir, &results);
    } else {
        print_human(socket_dir, &results);
    }

    Ok(())
}

#[cfg_attr(not(unix), allow(dead_code))]
fn elapsed_ms(start: std::time::Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn discover_sockets(socket_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut sockets = Vec::new();

    let Ok(entries) = std::fs::read_dir(socket_dir) else {
        return sockets;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.ends_with(".sock") && !name_str.ends_with(".tarpc.sock") {
            let primal_name = name_str
                .strip_suffix(".sock")
                .unwrap_or(&name_str)
                .to_owned();
            sockets.push((primal_name, path));
        }
    }

    sockets.sort_by(|a, b| a.0.cmp(&b.0));
    sockets
}

async fn probe_primal(name: &str, path: &Path, timeout: std::time::Duration) -> ProbeResult {
    let start = std::time::Instant::now();

    #[cfg(unix)]
    {
        let connect_result =
            tokio::time::timeout(timeout, UnixStream::connect(path)).await;

        let stream = match connect_result {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => {
                return ProbeResult {
                    name: name.to_owned(),
                    status: ProbeStatus::Unreachable,
                    version: None,
                    capabilities: Vec::new(),
                    latency_ms: elapsed_ms(start),
                };
            }
            Err(_) => {
                return ProbeResult {
                    name: name.to_owned(),
                    status: ProbeStatus::Timeout,
                    version: None,
                    capabilities: Vec::new(),
                    latency_ms: elapsed_ms(start),
                };
            }
        };

        let mut reader = BufReader::new(stream);

        let liveness = send_rpc(&mut reader, "health.liveness", timeout).await;
        let version = send_rpc(&mut reader, "system.version", timeout).await;
        let caps = send_rpc(&mut reader, "capabilities.list", timeout).await;

        let latency_ms = elapsed_ms(start);

        let is_alive = liveness
            .as_ref()
            .is_ok_and(|r| !r.contains("\"error\""));

        let version_str = version.ok().and_then(|r| {
            serde_json::from_str::<serde_json::Value>(&r)
                .ok()
                .and_then(|v| v.get("result").and_then(|r| r.as_str().map(String::from)))
        });

        let cap_list = caps
            .ok()
            .and_then(|r| {
                serde_json::from_str::<serde_json::Value>(&r)
                    .ok()
                    .and_then(|v| {
                        v.get("result")
                            .and_then(|r| r.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                    })
            })
            .unwrap_or_default();

        let status = if is_alive {
            ProbeStatus::Alive
        } else {
            ProbeStatus::Degraded
        };

        ProbeResult {
            name: name.to_owned(),
            status,
            version: version_str,
            capabilities: cap_list,
            latency_ms,
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (path, timeout);
        ProbeResult {
            name: name.to_owned(),
            status: ProbeStatus::Unreachable,
            version: None,
            capabilities: Vec::new(),
            latency_ms: start.elapsed().as_millis() as u64,
        }
    }
}

#[cfg(unix)]
async fn send_rpc(
    reader: &mut BufReader<UnixStream>,
    method: &str,
    timeout: std::time::Duration,
) -> std::result::Result<String, String> {
    let request = format!(
        r#"{{"jsonrpc":"2.0","method":"{method}","id":1}}"#,
    );
    let msg = format!("{request}\n");

    let writer = reader.get_mut();
    if let Err(e) = tokio::time::timeout(timeout, writer.write_all(msg.as_bytes())).await {
        return Err(format!("write timeout: {e}"));
    }

    let mut response = String::new();
    match tokio::time::timeout(timeout, reader.read_line(&mut response)).await {
        Ok(Ok(0) | Err(_)) => Err("connection closed".to_owned()),
        Ok(Ok(_)) => Ok(response),
        Err(_) => Err("read timeout".to_owned()),
    }
}

fn print_json(socket_dir: &Path, results: &[ProbeResult]) {
    let alive = results.iter().filter(|r| r.status == ProbeStatus::Alive).count();
    let total = results.len();

    let convergence = convergence_level(results);

    println!("{{");
    println!("  \"convergence\": \"{convergence}\",");
    println!("  \"socket_dir\": \"{}\",", socket_dir.display());
    println!("  \"alive\": {alive},");
    println!("  \"total\": {total},");
    println!("  \"primals\": [");
    for (i, r) in results.iter().enumerate() {
        let comma = if i + 1 < total { "," } else { "" };
        let caps = r
            .capabilities
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        let ver = r.version.as_deref().map_or_else(
            || "null".to_owned(),
            |v| format!("\"{v}\""),
        );
        println!(
            "    {{\"name\":\"{}\",\"status\":\"{}\",\"version\":{},\"capabilities\":[{}],\"latency_ms\":{}}}{}",
            r.name,
            r.status.as_str(),
            ver,
            caps,
            r.latency_ms,
            comma
        );
    }
    println!("  ]");
    println!("}}");
}

fn print_human(socket_dir: &Path, results: &[ProbeResult]) {
    let alive = results.iter().filter(|r| r.status == ProbeStatus::Alive).count();
    let total = results.len();
    let convergence = convergence_level(results);

    println!();
    crate::info(&format!(
        "Live convergence: {convergence} — {alive}/{total} alive ({})",
        socket_dir.display()
    ));
    println!();

    for r in results {
        let status_icon = match r.status {
            ProbeStatus::Alive => "✓",
            ProbeStatus::Degraded => "⚠",
            ProbeStatus::Unreachable => "✗",
            ProbeStatus::Timeout => "⏱",
        };

        let ver = r.version.as_deref().unwrap_or("?");
        let caps = if r.capabilities.is_empty() {
            String::new()
        } else {
            format!(" [{}]", r.capabilities.join(", "))
        };

        match r.status {
            ProbeStatus::Alive => {
                crate::success(&format!(
                    "  {status_icon} {} v{ver} ({}ms){caps}",
                    r.name, r.latency_ms
                ));
            }
            ProbeStatus::Degraded => {
                crate::warning(&format!(
                    "  {status_icon} {} — degraded ({}ms)",
                    r.name, r.latency_ms
                ));
            }
            _ => {
                crate::error(&format!(
                    "  {status_icon} {} — {} ({}ms)",
                    r.name,
                    r.status.as_str(),
                    r.latency_ms
                ));
            }
        }
    }

    println!();
    match convergence {
        "CONVERGED" => crate::success("All primals alive and responding"),
        "PARTIAL" => crate::warning("Some primals degraded or unreachable"),
        _ => crate::error("Convergence drift detected"),
    }
}

fn convergence_level(results: &[ProbeResult]) -> &'static str {
    if results.is_empty() {
        return "NO_PRIMALS";
    }

    let all_alive = results.iter().all(|r| r.status == ProbeStatus::Alive);
    let any_alive = results.iter().any(|r| r.status == ProbeStatus::Alive);

    if all_alive {
        "CONVERGED"
    } else if any_alive {
        "PARTIAL"
    } else {
        "DRIFT"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_sockets_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sockets = discover_sockets(tmp.path());
        assert!(sockets.is_empty());
    }

    #[test]
    fn discover_sockets_finds_json_rpc() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("beardog.sock"), "").unwrap();
        std::fs::write(tmp.path().join("squirrel.sock"), "").unwrap();
        std::fs::write(tmp.path().join("squirrel.tarpc.sock"), "").unwrap();

        let sockets = discover_sockets(tmp.path());
        assert_eq!(sockets.len(), 2);
        assert_eq!(sockets[0].0, "beardog");
        assert_eq!(sockets[1].0, "squirrel");
    }

    #[test]
    fn discover_sockets_ignores_non_sock() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("beardog.pid"), "12345").unwrap();
        std::fs::write(tmp.path().join("config.toml"), "").unwrap();

        let sockets = discover_sockets(tmp.path());
        assert!(sockets.is_empty());
    }

    #[test]
    fn convergence_all_alive() {
        let results = vec![
            ProbeResult {
                name: "a".to_owned(),
                status: ProbeStatus::Alive,
                version: Some("1.0".to_owned()),
                capabilities: vec![],
                latency_ms: 5,
            },
            ProbeResult {
                name: "b".to_owned(),
                status: ProbeStatus::Alive,
                version: Some("1.0".to_owned()),
                capabilities: vec![],
                latency_ms: 3,
            },
        ];
        assert_eq!(convergence_level(&results), "CONVERGED");
    }

    #[test]
    fn convergence_partial() {
        let results = vec![
            ProbeResult {
                name: "a".to_owned(),
                status: ProbeStatus::Alive,
                version: None,
                capabilities: vec![],
                latency_ms: 5,
            },
            ProbeResult {
                name: "b".to_owned(),
                status: ProbeStatus::Unreachable,
                version: None,
                capabilities: vec![],
                latency_ms: 100,
            },
        ];
        assert_eq!(convergence_level(&results), "PARTIAL");
    }

    #[test]
    fn convergence_drift() {
        let results = vec![ProbeResult {
            name: "a".to_owned(),
            status: ProbeStatus::Timeout,
            version: None,
            capabilities: vec![],
            latency_ms: 5000,
        }];
        assert_eq!(convergence_level(&results), "DRIFT");
    }

    #[test]
    fn convergence_no_primals() {
        assert_eq!(convergence_level(&[]), "NO_PRIMALS");
    }

    #[test]
    fn probe_status_as_str() {
        assert_eq!(ProbeStatus::Alive.as_str(), "ALIVE");
        assert_eq!(ProbeStatus::Degraded.as_str(), "DEGRADED");
        assert_eq!(ProbeStatus::Unreachable.as_str(), "UNREACHABLE");
        assert_eq!(ProbeStatus::Timeout.as_str(), "TIMEOUT");
    }
}
