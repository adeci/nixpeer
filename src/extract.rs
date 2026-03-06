use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use thiserror::Error;

/// Summary of system-level artifacts extracted from a NixOS configuration.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SystemSummary {
    pub systemd_services: BTreeMap<String, ServiceInfo>,
    pub systemd_timers: Vec<String>,
    pub users: BTreeMap<String, UserInfo>,
    pub groups: Vec<String>,
    pub firewall: FirewallInfo,
    pub nginx_vhosts: Vec<String>,
    pub environment_packages: Vec<String>,
    pub etc_files: Vec<String>,
    pub postgresql: PostgresqlInfo,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct ServiceInfo {
    pub description: String,
    pub wanted_by: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct UserInfo {
    pub uid: Option<u32>,
    pub group: String,
    pub is_system_user: bool,
    pub is_normal_user: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct FirewallInfo {
    pub enable: bool,
    pub allowed_tcp_ports: Vec<u16>,
    pub allowed_udp_ports: Vec<u16>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct PostgresqlInfo {
    pub enable: bool,
    pub ensure_databases: Vec<String>,
    pub ensure_users: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("failed to run nix: {0}")]
    NixExec(#[from] std::io::Error),

    #[error("nix eval failed:\n{0}")]
    NixEval(String),

    #[error("failed to parse nix output: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("not a NixOS system (no /run/current-system)")]
    NotNixOS,
}

/// Evaluate a NixOS configuration and extract its system summary.
pub fn extract(flake_ref: &str, extractor_path: &Path) -> Result<SystemSummary, ExtractError> {
    let output = Command::new("nix")
        .args([
            "eval",
            "--json",
            "--impure",
            &format!("{flake_ref}.config"),
            "--apply",
            &format!("import {}", extractor_path.display()),
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ExtractError::NixEval(stderr.to_string()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(serde_json::from_str(&stdout)?)
}
