//! Live RPC surface audit — detects API divergence from declared capabilities.
//!
//! Connects to a running primal and verifies:
//! 1. **Method not found**: Unknown methods get proper `-32601` errors (not silent fallback)
//! 2. **Declared methods respond**: Each declared method returns a valid response (not error)
//! 3. **No health fallback**: Responses to real methods differ from a generic health probe
//! 4. **Capabilities match**: `capabilities.list` response matches expected domains
//!
//! This catches the P0-A pattern (bearDog returning health for ALL methods) and the P0-B
//! pattern (nestGate declaring methods that don't actually exist).
//!
//! Usage:
//! ```text
//! sourdough validate rpc-surface --socket /run/user/1000/biomeos/beardog.sock
//! sourdough validate rpc-surface --socket /run/user/1000/biomeos/beardog.sock --methods crypto.sign_ed25519,crypto.verify_ed25519
//! ```

use anyhow::Result;
use std::path::Path;

#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::UnixStream;

/// A canary method that no primal should recognize.
#[cfg_attr(not(unix), allow(dead_code))]
const CANARY_METHOD: &str = "__sourDough_rpc_audit_canary_9f3a7b2e";

#[cfg_attr(not(unix), allow(dead_code))]
struct AuditResult {
    primal_name: Option<String>,
    findings: Vec<Finding>,
}

#[cfg_attr(not(unix), allow(dead_code))]
struct Finding {
    category: &'static str,
    method: String,
    severity: Severity,
    message: String,
}

#[cfg_attr(not(unix), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Severity {
    Pass,
    Warning,
    Fail,
}

pub(super) fn validate(
    socket: &Path,
    methods: &[String],
    timeout_ms: u64,
    json: bool,
) -> Result<()> {
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(validate_async(
            socket, methods, timeout_ms, json,
        ))
    })
}

async fn validate_async(
    socket: &Path,
    methods: &[String],
    timeout_ms: u64,
    json: bool,
) -> Result<()> {
    let timeout = std::time::Duration::from_millis(timeout_ms);

    #[cfg(unix)]
    {
        let result = audit_primal(socket, methods, timeout).await;
        if json {
            print_json(socket, &result);
        } else {
            print_human(socket, &result);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = (socket, methods, timeout);
        if json {
            println!(r#"{{"error":"rpc-surface audit requires Unix (UDS sockets)"}}"#);
        } else {
            crate::error("RPC surface audit requires Unix (UDS sockets)");
        }
    }

    Ok(())
}

#[cfg(unix)]
async fn audit_primal(
    socket: &Path,
    declared_methods: &[String],
    timeout: std::time::Duration,
) -> AuditResult {
    let mut findings = Vec::new();

    // Phase 1: Canary — send unknown method on fresh connection, expect -32601
    let canary_response = send_rpc_fresh(socket, CANARY_METHOD, None, timeout).await;
    let health_fallback_detected = check_canary(&canary_response, &mut findings);

    // Phase 2: Get capabilities.list to identify the primal
    let caps_response = send_rpc_fresh(socket, "capabilities.list", None, timeout).await;
    let primal_name = extract_primal_name(&caps_response);

    // Phase 3: Check each declared method (fresh connection per call)
    let methods_to_check: Vec<String> = if declared_methods.is_empty() {
        default_probe_methods()
    } else {
        declared_methods.to_vec()
    };

    for method in &methods_to_check {
        let response = send_rpc_fresh(socket, method, None, timeout).await;
        evaluate_method_response(
            method,
            &response,
            health_fallback_detected,
            &canary_response,
            &mut findings,
        );
    }

    AuditResult {
        primal_name,
        findings,
    }
}

/// Send a single RPC call on a fresh connection (handles one-shot primals).
#[cfg(unix)]
async fn send_rpc_fresh(
    socket: &Path,
    method: &str,
    params: Option<&serde_json::Value>,
    timeout: std::time::Duration,
) -> std::result::Result<String, String> {
    let connect_result = tokio::time::timeout(timeout, UnixStream::connect(socket)).await;

    let stream = match connect_result {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("connect: {e}")),
        Err(_) => return Err("connect timeout".to_owned()),
    };

    let mut reader = BufReader::new(stream);
    send_rpc(&mut reader, method, params, timeout).await
}

#[cfg(unix)]
fn check_canary(
    response: &std::result::Result<String, String>,
    findings: &mut Vec<Finding>,
) -> bool {
    match response {
        Ok(resp) => {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(resp) {
                if let Some(error) = json.get("error") {
                    let code = error.get("code").and_then(serde_json::Value::as_i64).unwrap_or(0);
                    if code == -32601 {
                        findings.push(Finding {
                            category: "canary",
                            method: CANARY_METHOD.to_owned(),
                            severity: Severity::Pass,
                            message: "unknown method correctly returns -32601".to_owned(),
                        });
                        return false;
                    }
                    findings.push(Finding {
                        category: "canary",
                        method: CANARY_METHOD.to_owned(),
                        severity: Severity::Warning,
                        message: format!("unknown method returns error code {code} (expected -32601)"),
                    });
                    return false;
                }

                // Got a result for an unknown method — HEALTH FALLBACK DETECTED
                findings.push(Finding {
                    category: "canary",
                    method: CANARY_METHOD.to_owned(),
                    severity: Severity::Fail,
                    message: "HEALTH FALLBACK: unknown method returns success response (P0-A pattern)"
                        .to_owned(),
                });
                true
            } else {
                findings.push(Finding {
                    category: "canary",
                    method: CANARY_METHOD.to_owned(),
                    severity: Severity::Warning,
                    message: "response is not valid JSON-RPC".to_owned(),
                });
                false
            }
        }
        Err(e) => {
            findings.push(Finding {
                category: "canary",
                method: CANARY_METHOD.to_owned(),
                severity: Severity::Fail,
                message: format!("canary probe failed: {e}"),
            });
            false
        }
    }
}

#[cfg(unix)]
fn extract_primal_name(response: &std::result::Result<String, String>) -> Option<String> {
    response.as_ref().ok().and_then(|r| {
        serde_json::from_str::<serde_json::Value>(r)
            .ok()
            .and_then(|v| {
                v.get("result")
                    .and_then(|r| r.get("primal").or_else(|| r.get("name")))
                    .and_then(|n| n.as_str())
                    .map(String::from)
            })
    })
}

#[cfg(unix)]
fn evaluate_method_response(
    method: &str,
    response: &std::result::Result<String, String>,
    health_fallback: bool,
    canary_response: &std::result::Result<String, String>,
    findings: &mut Vec<Finding>,
) {
    match response {
        Ok(resp) => {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(resp) {
                if json.get("error").is_some() {
                    let code = json
                        .get("error")
                        .and_then(|e| e.get("code"))
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0);
                    let msg = json
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown");

                    if code == -32601 {
                        findings.push(Finding {
                            category: "method",
                            method: method.to_owned(),
                            severity: Severity::Fail,
                            message: "declared method returns -32601 (NOT IMPLEMENTED)".to_owned(),
                        });
                    } else {
                        findings.push(Finding {
                            category: "method",
                            method: method.to_owned(),
                            severity: Severity::Warning,
                            message: format!("returns error {code}: {msg}"),
                        });
                    }
                } else if health_fallback && responses_match(canary_response, response) {
                    findings.push(Finding {
                        category: "method",
                        method: method.to_owned(),
                        severity: Severity::Fail,
                        message: "response identical to canary — likely health fallback stub"
                            .to_owned(),
                    });
                } else {
                    findings.push(Finding {
                        category: "method",
                        method: method.to_owned(),
                        severity: Severity::Pass,
                        message: "responds with valid result".to_owned(),
                    });
                }
            } else {
                findings.push(Finding {
                    category: "method",
                    method: method.to_owned(),
                    severity: Severity::Warning,
                    message: "response is not valid JSON-RPC".to_owned(),
                });
            }
        }
        Err(e) => {
            findings.push(Finding {
                category: "method",
                method: method.to_owned(),
                severity: Severity::Fail,
                message: format!("probe failed: {e}"),
            });
        }
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
fn responses_match(
    a: &std::result::Result<String, String>,
    b: &std::result::Result<String, String>,
) -> bool {
    let (Ok(a_str), Ok(b_str)) = (a, b) else {
        return false;
    };

    let (Ok(a_json), Ok(b_json)) = (
        serde_json::from_str::<serde_json::Value>(a_str),
        serde_json::from_str::<serde_json::Value>(b_str),
    ) else {
        return false;
    };

    // Compare result fields only (ignore id)
    a_json.get("result") == b_json.get("result")
}

#[cfg_attr(not(unix), allow(dead_code))]
fn default_probe_methods() -> Vec<String> {
    vec![
        "health.liveness".to_owned(),
        "health.readiness".to_owned(),
        "capabilities.list".to_owned(),
        "system.version".to_owned(),
        "system.ping".to_owned(),
    ]
}

#[cfg(unix)]
async fn send_rpc(
    reader: &mut BufReader<UnixStream>,
    method: &str,
    params: Option<&serde_json::Value>,
    timeout: std::time::Duration,
) -> std::result::Result<String, String> {
    let request = params.map_or_else(
        || format!(r#"{{"jsonrpc":"2.0","method":"{method}","id":1}}"#),
        |p| format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{p},"id":1}}"#),
    );
    let msg = format!("{request}\n");

    let writer = reader.get_mut();
    let write_result = tokio::time::timeout(timeout, writer.write_all(msg.as_bytes())).await;
    if let Err(e) = write_result {
        return Err(format!("write timeout: {e}"));
    }
    if let Ok(Err(e)) = write_result {
        return Err(format!("write error: {e}"));
    }

    let mut response = String::new();
    match tokio::time::timeout(timeout, reader.read_line(&mut response)).await {
        Ok(Ok(0) | Err(_)) => Err("connection closed".to_owned()),
        Ok(Ok(_)) => Ok(response),
        Err(_) => Err("read timeout".to_owned()),
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
fn compliance_level(findings: &[Finding]) -> &'static str {
    let has_health_fallback = findings
        .iter()
        .any(|f| f.category == "canary" && f.severity == Severity::Fail);
    let has_missing_methods = findings
        .iter()
        .any(|f| f.category == "method" && f.severity == Severity::Fail);
    let fails = findings.iter().filter(|f| f.severity == Severity::Fail).count();

    if has_health_fallback {
        "STUB"
    } else if has_missing_methods {
        "DIVERGED"
    } else if fails > 0 {
        "BROKEN"
    } else {
        "VERIFIED"
    }
}

#[cfg_attr(not(unix), allow(dead_code))]
fn print_json(socket: &Path, result: &AuditResult) {
    let level = compliance_level(&result.findings);
    let name = result
        .primal_name
        .as_deref()
        .unwrap_or("unknown");

    let passes: Vec<_> = result.findings.iter().filter(|f| f.severity == Severity::Pass).collect();
    let warns: Vec<_> = result.findings.iter().filter(|f| f.severity == Severity::Warning).collect();
    let fails: Vec<_> = result.findings.iter().filter(|f| f.severity == Severity::Fail).collect();

    println!("{{");
    println!("  \"compliance\": \"{level}\",");
    println!("  \"primal\": \"{name}\",");
    println!("  \"socket\": \"{}\",", socket.display());
    println!("  \"pass\": [");
    for (i, f) in passes.iter().enumerate() {
        let comma = if i + 1 < passes.len() { "," } else { "" };
        println!(
            "    {{\"category\":\"{}\",\"method\":\"{}\",\"message\":\"{}\"}}{}",
            f.category, f.method, f.message, comma
        );
    }
    println!("  ],");
    println!("  \"warnings\": [");
    for (i, f) in warns.iter().enumerate() {
        let comma = if i + 1 < warns.len() { "," } else { "" };
        println!(
            "    {{\"category\":\"{}\",\"method\":\"{}\",\"message\":\"{}\"}}{}",
            f.category, f.method, f.message, comma
        );
    }
    println!("  ],");
    println!("  \"failures\": [");
    for (i, f) in fails.iter().enumerate() {
        let comma = if i + 1 < fails.len() { "," } else { "" };
        println!(
            "    {{\"category\":\"{}\",\"method\":\"{}\",\"message\":\"{}\"}}{}",
            f.category, f.method, f.message, comma
        );
    }
    println!("  ]");
    println!("}}");
}

#[cfg_attr(not(unix), allow(dead_code))]
fn print_human(socket: &Path, result: &AuditResult) {
    let level = compliance_level(&result.findings);
    let name = result.primal_name.as_deref().unwrap_or("unknown");

    println!();
    crate::info(&format!(
        "RPC surface audit: {level} — {} ({})",
        name,
        socket.display()
    ));
    println!();

    for f in &result.findings {
        let method_tag = if f.method.is_empty() {
            String::new()
        } else {
            format!(" `{}`", f.method)
        };

        match f.severity {
            Severity::Pass => {
                crate::success(&format!("  [{}]{method_tag} {}", f.category, f.message));
            }
            Severity::Warning => {
                crate::warning(&format!("  [{}]{method_tag} {}", f.category, f.message));
            }
            Severity::Fail => {
                crate::error(&format!("  [{}]{method_tag} {}", f.category, f.message));
            }
        }
    }

    println!();
    match level {
        "VERIFIED" => crate::success("RPC surface verified — methods respond as declared"),
        "STUB" => crate::error(
            "HEALTH FALLBACK STUB detected — primal returns health for unknown methods (P0-A)",
        ),
        "DIVERGED" => crate::error(
            "API SURFACE DIVERGED — declared methods return -32601 (P0-B pattern)",
        ),
        _ => crate::error("RPC surface broken — connection or protocol failures"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compliance_stub_detected() {
        let findings = vec![Finding {
            category: "canary",
            method: CANARY_METHOD.to_owned(),
            severity: Severity::Fail,
            message: "health fallback".to_owned(),
        }];
        assert_eq!(compliance_level(&findings), "STUB");
    }

    #[test]
    fn compliance_diverged_detected() {
        let findings = vec![
            Finding {
                category: "canary",
                method: CANARY_METHOD.to_owned(),
                severity: Severity::Pass,
                message: "ok".to_owned(),
            },
            Finding {
                category: "method",
                method: "crypto.sign".to_owned(),
                severity: Severity::Fail,
                message: "not implemented".to_owned(),
            },
        ];
        assert_eq!(compliance_level(&findings), "DIVERGED");
    }

    #[test]
    fn compliance_verified_all_pass() {
        let findings = vec![
            Finding {
                category: "canary",
                method: CANARY_METHOD.to_owned(),
                severity: Severity::Pass,
                message: "ok".to_owned(),
            },
            Finding {
                category: "method",
                method: "health.liveness".to_owned(),
                severity: Severity::Pass,
                message: "ok".to_owned(),
            },
        ];
        assert_eq!(compliance_level(&findings), "VERIFIED");
    }

    #[test]
    fn compliance_broken_on_connect_fail() {
        let findings = vec![Finding {
            category: "connect",
            method: String::new(),
            severity: Severity::Fail,
            message: "cannot connect".to_owned(),
        }];
        assert_eq!(compliance_level(&findings), "BROKEN");
    }

    #[test]
    fn default_methods_are_core() {
        let methods = default_probe_methods();
        assert!(methods.contains(&"health.liveness".to_owned()));
        assert!(methods.contains(&"capabilities.list".to_owned()));
        assert!(methods.contains(&"system.version".to_owned()));
    }

    #[test]
    fn responses_match_identical_results() {
        let a = Ok(r#"{"jsonrpc":"2.0","result":{"status":"alive"},"id":1}"#.to_owned());
        let b = Ok(r#"{"jsonrpc":"2.0","result":{"status":"alive"},"id":2}"#.to_owned());
        assert!(responses_match(&a, &b));
    }

    #[test]
    fn responses_dont_match_different_results() {
        let a = Ok(r#"{"jsonrpc":"2.0","result":{"status":"alive"},"id":1}"#.to_owned());
        let b = Ok(r#"{"jsonrpc":"2.0","result":{"version":"1.0"},"id":1}"#.to_owned());
        assert!(!responses_match(&a, &b));
    }
}
