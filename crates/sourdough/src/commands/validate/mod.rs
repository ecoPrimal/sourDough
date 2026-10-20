//! Validation commands for checking primal compliance.

mod composition;
mod convergence;
mod deps;
mod depot;
mod ecobin;
mod mass;
mod neural_api;
mod platform_paths;
mod platform_substrate;
pub(crate) mod ribocipher;
mod rpc_surface;
mod tarpc_compliance;
mod transport_compliance;
mod transport_report;

use anyhow::Result;
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(crate) enum ValidateCommand {
    /// Validate basic primal structure
    Primal {
        /// Path to the primal directory
        path: PathBuf,
    },

    /// Validate `UniBin` compliance
    #[command(name = "unibin")]
    UniBin {
        /// Path to the primal directory
        path: PathBuf,
    },

    /// Validate `ecoBin` compliance (project dir or compiled binary)
    #[command(name = "ecobin")]
    EcoBin {
        /// Path to the primal directory or compiled binary
        path: PathBuf,
    },

    /// Validate a primal composition (check binary presence)
    #[command(name = "composition")]
    Composition {
        /// Composition name (tower, node, nest, nucleus, full) or comma-separated primal list
        composition: String,

        /// Path to the primals binary directory
        #[arg(long, default_value = "primals")]
        primals_dir: PathBuf,

        /// Use triple-first layout (`primals/{triple}/{name}`)
        #[arg(long)]
        triple_first: bool,

        /// Path to a custom compositions manifest (TOML)
        #[arg(long)]
        manifest: Option<PathBuf>,
    },

    /// Validate transport abstraction compliance
    #[command(name = "transport")]
    Transport {
        /// Path to the primal directory
        path: PathBuf,
    },

    /// Check depot binary freshness (detect stale binaries)
    #[command(name = "depot")]
    Depot {
        /// Path to the depot directory (primals binary tree)
        #[arg(default_value = "primals")]
        depot_dir: PathBuf,

        /// Source directory to compare against (uses latest file mtime)
        #[arg(long)]
        source: Option<PathBuf>,

        /// Hours after which a binary is considered stale (default: 48)
        #[arg(long, default_value = "48")]
        stale_hours: u64,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Validate riboCipher transport signal compliance (Wave 111)
    #[command(name = "ribocipher")]
    RiboCipher {
        /// Path to the primal directory to audit
        path: PathBuf,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Validate tarpc dual-protocol compliance (G64 Cephalization)
    #[command(name = "tarpc")]
    Tarpc {
        /// Path to the primal directory to audit
        path: PathBuf,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Validate G68 platform substrate compliance (links, permissions, device backends)
    #[command(name = "platform-substrate")]
    PlatformSubstrate {
        /// Path to the primal directory to audit
        path: PathBuf,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Validate platform paths compliance (detect hardcoded path assumptions)
    #[command(name = "platform-paths")]
    PlatformPaths {
        /// Path to the primal directory to audit
        path: PathBuf,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Validate Neural API routing compliance (atomic routing matrix)
    #[command(name = "neural-api")]
    NeuralApi {
        /// Path to the primal directory to audit
        path: PathBuf,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Live RPC surface audit — detect API divergence (P0-A/P0-B patterns)
    #[command(name = "rpc-surface")]
    RpcSurface {
        /// Socket path of the primal to audit
        #[arg(long)]
        socket: PathBuf,

        /// Methods to probe (comma-separated). Defaults to core health/caps/version.
        #[arg(long, value_delimiter = ',')]
        methods: Vec<String>,

        /// Per-method probe timeout in milliseconds
        #[arg(long, default_value = "2000")]
        timeout_ms: u64,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Live convergence check — probe running primals via sockets
    #[command(name = "convergence")]
    Convergence {
        /// Socket directory to scan for running primals
        #[arg(long)]
        socket_dir: Option<PathBuf>,

        /// Per-primal probe timeout in milliseconds
        #[arg(long, default_value = "2000")]
        timeout_ms: u64,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Stoichiometric mass sort — Shannon entropy page weights for Zola sites
    #[command(name = "mass")]
    Mass {
        /// Path to the Zola site content directory
        #[arg(default_value = ".")]
        content_dir: PathBuf,

        /// Actually write weights into frontmatter (default: dry run)
        #[arg(long)]
        apply: bool,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// G72 Dependency Pandemic audit (bloated features, excisable crates, version drift)
    #[command(name = "deps")]
    Deps {
        /// Path to the primal directory to audit
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },

    /// Run transport compliance audit across all primals in a directory
    #[command(name = "transport-report")]
    TransportReport {
        /// Parent directory containing primal checkouts
        #[arg(long, default_value = "..")]
        primals_dir: PathBuf,

        /// Write report to file (in addition to stdout)
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Output as JSON (machine-readable for CI/depot automation)
        #[arg(long)]
        json: bool,

        /// Exempt primals (comma-separated, e.g. "biomeOS,songBird,sourDough")
        #[arg(
            long,
            value_delimiter = ',',
            default_value = "biomeOS,songBird,sourDough"
        )]
        exempt: Vec<String>,
    },
}

pub(crate) fn run(cmd: ValidateCommand) -> Result<()> {
    match cmd {
        ValidateCommand::Primal { path } => validate_primal(&path),
        ValidateCommand::UniBin { path } => validate_unibin(&path),
        ValidateCommand::EcoBin { path } => ecobin::validate(&path),
        ValidateCommand::Composition {
            composition,
            primals_dir,
            triple_first,
            manifest,
        } => composition::validate(
            &composition,
            &primals_dir,
            triple_first,
            manifest.as_deref(),
        ),
        ValidateCommand::Transport { path } => transport_compliance::validate(&path),
        ValidateCommand::RiboCipher { path, json } => ribocipher::run(&path, json),
        ValidateCommand::Tarpc { path, json } => tarpc_compliance::validate(&path, json),
        ValidateCommand::PlatformSubstrate { path, json } => {
            platform_substrate::validate(&path, json)
        }
        ValidateCommand::PlatformPaths { path, json } => {
            platform_paths::validate(&path, json)
        }
        ValidateCommand::NeuralApi { path, json } => neural_api::validate(&path, json),
        ValidateCommand::RpcSurface {
            socket,
            methods,
            timeout_ms,
            json,
        } => rpc_surface::validate(&socket, &methods, timeout_ms, json),
        ValidateCommand::Convergence {
            socket_dir,
            timeout_ms,
            json,
        } => {
            let dir = socket_dir.unwrap_or_else(default_socket_dir);
            convergence::validate(&dir, json, timeout_ms)
        }
        ValidateCommand::Mass {
            content_dir,
            apply,
            json,
        } => mass::validate(&content_dir, json, apply),
        ValidateCommand::Deps { path, json } => deps::validate(&path, json),
        ValidateCommand::Depot {
            depot_dir,
            source,
            stale_hours,
            json,
        } => depot::run(&depot_dir, source.as_deref(), stale_hours, json),
        ValidateCommand::TransportReport {
            primals_dir,
            output,
            json,
            exempt,
        } => transport_report::run(&primals_dir, output.as_deref(), json, &exempt),
    }
}

fn default_socket_dir() -> PathBuf {
    let runtime_dir = std::env::var("BIOMEOS_SOCKET_DIR")
        .or_else(|_| {
            std::env::var("XDG_RUNTIME_DIR").map(|d| format!("{d}/biomeos"))
        })
        .unwrap_or_else(|_| "/tmp/biomeos".to_owned());
    PathBuf::from(runtime_dir)
}

// --- Shared validation functions ---

fn validate_primal(path: &Path) -> Result<()> {
    crate::info(&format!("Validating primal at: {}", path.display()));
    println!();

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if path.join("Cargo.toml").exists() {
        crate::success("Cargo.toml found");
    } else {
        errors.push("Missing Cargo.toml".to_string());
    }

    if path.join("specs").exists() {
        crate::success("specs/ directory found");
    } else {
        warnings.push("Missing specs/ directory".to_string());
    }

    if path.join("crates").exists() {
        crate::success("crates/ directory found");
    } else {
        errors.push("Missing crates/ directory".to_string());
    }

    if path.join("README.md").exists() {
        crate::success("README.md found");
    } else {
        warnings.push("Missing README.md".to_string());
    }

    let crates_dir = path.join("crates");
    if crates_dir.exists() {
        let entries: Vec<_> = std::fs::read_dir(&crates_dir)?
            .filter_map(std::result::Result::ok)
            .collect();

        let has_core = entries
            .iter()
            .any(|e| e.file_name().to_string_lossy().contains("-core"));

        if has_core {
            crate::success("Core crate found");
        } else {
            warnings.push("No *-core crate found".to_string());
        }
    }

    check_trait_implementations(path)?;

    println!();
    report_results(&errors, &warnings)
}

fn validate_unibin(path: &Path) -> Result<()> {
    crate::info(&format!("Validating UniBin at: {}", path.display()));
    println!();

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    crate::info("Running basic primal validation...");
    validate_primal(path)?;

    println!();
    crate::info("Checking UniBin compliance...");

    let cargo_toml_path = path.join("Cargo.toml");
    if cargo_toml_path.exists() {
        let content = std::fs::read_to_string(&cargo_toml_path)?;

        if content.contains("[[bin]]") {
            let bin_count = content.matches("[[bin]]").count();
            if bin_count == 1 {
                crate::success("Single binary defined");
            } else {
                errors.push("Multiple binaries defined (should be one UniBin)".to_string());
            }
        } else {
            warnings.push("No [[bin]] section found".to_string());
        }
    }

    println!();
    report_results(&errors, &warnings)
}

// --- Shared utilities (used by sub-modules) ---

pub(super) fn find_source_dir(path: &Path) -> Option<PathBuf> {
    let crates_dir = path.join("crates");
    if crates_dir.exists() {
        for entry in std::fs::read_dir(&crates_dir).ok()?.flatten() {
            let src = entry.path().join("src");
            if src.exists() {
                return Some(crates_dir);
            }
        }
    }
    let src = path.join("src");
    if src.exists() {
        return Some(src);
    }
    None
}

pub(super) fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files_recursive(dir, &mut files);
    files
}

fn collect_rs_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files_recursive(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn check_trait_implementations(path: &Path) -> Result<()> {
    crate::info("Checking trait implementations...");

    let crates_dir = path.join("crates");
    if !crates_dir.exists() {
        return Ok(());
    }

    let entries: Vec<_> = std::fs::read_dir(&crates_dir)?
        .filter_map(std::result::Result::ok)
        .collect();

    for entry in entries {
        let name = entry.file_name();
        if name.to_string_lossy().contains("-core") {
            let lib_rs = entry.path().join("src/lib.rs");
            if lib_rs.exists() {
                let content = std::fs::read_to_string(&lib_rs)?;

                let traits_to_check = [
                    ("PrimalLifecycle", "lifecycle management"),
                    ("PrimalHealth", "health reporting"),
                    ("PrimalIdentity", "identity (via universal adapter)"),
                    ("PrimalDiscovery", "discovery (via universal adapter)"),
                ];

                for (trait_name, description) in traits_to_check {
                    if content.contains(trait_name) {
                        crate::success(&format!("  {trait_name} implemented ({description})"));
                    }
                }
            }
        }
    }

    Ok(())
}

fn report_results(errors: &[String], warnings: &[String]) -> Result<()> {
    if !errors.is_empty() {
        println!();
        crate::error("Validation errors:");
        for error in errors {
            println!("  ✗ {error}");
        }
    }

    if !warnings.is_empty() {
        println!();
        crate::warning("Warnings:");
        for warning in warnings {
            println!("  ⚠ {warning}");
        }
    }

    if errors.is_empty() {
        println!();
        if warnings.is_empty() {
            crate::success("All checks passed!");
        } else {
            crate::success("Validation passed (with warnings)");
        }
        Ok(())
    } else {
        let n = errors.len();
        anyhow::bail!("Validation failed with {n} error(s)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_source_dir_with_crates() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_src = tmp.path().join("crates/foo/src");
        std::fs::create_dir_all(&crate_src).unwrap();
        let result = find_source_dir(tmp.path());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("crates"));
    }

    #[test]
    fn find_source_dir_with_src() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let result = find_source_dir(tmp.path());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("src"));
    }

    #[test]
    fn find_source_dir_missing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(find_source_dir(tmp.path()).is_none());
    }

    #[test]
    fn collect_rs_files_finds_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("sub/deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn a(){}").unwrap();
        std::fs::write(nested.join("b.rs"), "fn b(){}").unwrap();
        std::fs::write(tmp.path().join("not_rs.txt"), "text").unwrap();
        let files = collect_rs_files(tmp.path());
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|f| f.extension().unwrap() == "rs"));
    }

    #[test]
    fn report_results_ok_with_no_errors() {
        assert!(report_results(&[], &[]).is_ok());
    }

    #[test]
    fn report_results_fails_with_errors() {
        let result = report_results(&["oops".to_string()], &[]);
        assert!(result.is_err());
    }
}
