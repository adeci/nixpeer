use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Machine identity metadata.
#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct MachineInfo {
    pub hostname: String,
    pub nixos_version: String,
    pub system: String,
}

impl MachineInfo {
    /// Short display label: "hostname (version)" or just "hostname".
    pub fn label(&self) -> String {
        if self.nixos_version.is_empty() {
            self.hostname.clone()
        } else {
            let short_ver = self
                .nixos_version
                .split('.')
                .take(3)
                .collect::<Vec<_>>()
                .join(".");
            format!("{} ({})", self.hostname, short_ver)
        }
    }
}

/// Summary of system-level artifacts extracted from a NixOS system closure.
///
/// These are the core NixOS primitives. Every change to a NixOS system
/// (services, packages, config files, users, firewall rules) flows through
/// one of these.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SystemSummary {
    #[serde(default)]
    pub machine: MachineInfo,
    pub systemd_services: BTreeMap<String, ServiceInfo>,
    pub systemd_timers: Vec<String>,
    pub users: BTreeMap<String, UserInfo>,
    pub groups: Vec<String>,
    pub firewall: FirewallInfo,
    pub environment_packages: Vec<String>,
    /// Etc files mapped to their store path (for detecting modifications).
    pub etc_files: BTreeMap<String, String>,
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

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("failed to run command: {0}")]
    Exec(#[from] std::io::Error),

    #[error("not a NixOS system (no /run/current-system)")]
    NotNixOS,

    #[error("generation {0} not found")]
    GenerationNotFound(u64),
}
