//! Extract a SystemSummary from the currently running NixOS system.
//!
//! Reads exclusively from the nix store via `/run/current-system`. No root
//! access needed, no runtime state queried — just the declarative artifacts
//! that NixOS built.

use crate::extract::{
    ExtractError, FirewallInfo, MachineInfo, PostgresqlInfo, ServiceInfo, SystemSummary, UserInfo,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The system closure root — a symlink into the nix store.
const SYSTEM_ROOT: &str = "/run/current-system";

/// The system profile directory containing generation links.
const PROFILE_DIR: &str = "/nix/var/nix/profiles";

/// Extract a system summary from the live NixOS system.
pub fn extract_live() -> Result<SystemSummary, ExtractError> {
    extract_from_root(Path::new(SYSTEM_ROOT))
}

/// Extract a system summary from a specific generation number.
pub fn extract_generation(generation: u64) -> Result<SystemSummary, ExtractError> {
    let link = Path::new(PROFILE_DIR).join(format!("system-{generation}-link"));
    if !link.exists() {
        return Err(ExtractError::GenerationNotFound(generation));
    }
    extract_from_root(&link)
}

/// Extract a system summary from any system root path (current system or generation link).
fn extract_from_root(root: &Path) -> Result<SystemSummary, ExtractError> {
    if !root.exists() {
        return Err(ExtractError::NotNixOS);
    }

    let unit_dir = resolve_units_dir(root);
    let etc_dir = resolve_etc_dir(root);

    Ok(SystemSummary {
        machine: extract_machine_info(root, &etc_dir),
        systemd_services: extract_services(&unit_dir),
        systemd_timers: extract_timers(&unit_dir),
        users: extract_users(root),
        groups: extract_groups(root),
        firewall: extract_firewall(&unit_dir),
        nginx_vhosts: extract_nginx_vhosts(&etc_dir),
        environment_packages: extract_packages(root),
        etc_files: extract_etc_files(&etc_dir),
        postgresql: extract_postgresql(&unit_dir),
    })
}

/// Resolve the systemd system units directory from the store.
fn resolve_units_dir(root: &Path) -> std::path::PathBuf {
    // /run/current-system/etc/systemd/system → /nix/store/xxx-system-units
    root.join("etc/systemd/system")
}

/// Resolve the etc directory from the store.
fn resolve_etc_dir(root: &Path) -> std::path::PathBuf {
    // /run/current-system/etc → /nix/store/xxx-etc/etc
    root.join("etc")
}

fn extract_machine_info(root: &Path, etc_dir: &Path) -> MachineInfo {
    let hostname = fs::read_to_string(etc_dir.join("hostname"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let nixos_version = fs::read_to_string(root.join("nixos-version"))
        .unwrap_or_default()
        .trim()
        .to_string();
    let system = fs::read_to_string(root.join("system"))
        .unwrap_or_default()
        .trim()
        .to_string();

    MachineInfo {
        hostname,
        nixos_version,
        system,
    }
}

fn extract_services(unit_dir: &Path) -> BTreeMap<String, ServiceInfo> {
    let mut services = BTreeMap::new();

    let entries = match fs::read_dir(unit_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("warning: cannot read {}: {e}", unit_dir.display());
            return services;
        }
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(service_name) = name.strip_suffix(".service") else {
            continue;
        };

        let content = match fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut description = String::new();
        let mut wanted_by = Vec::new();
        let mut after = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("Description=") {
                description = val.to_string();
            } else if let Some(val) = line.strip_prefix("WantedBy=") {
                wanted_by.extend(val.split_whitespace().map(String::from));
            } else if let Some(val) = line.strip_prefix("After=") {
                after.extend(val.split_whitespace().map(String::from));
            }
        }

        services.insert(
            service_name.to_string(),
            ServiceInfo {
                description,
                wanted_by,
                after,
            },
        );
    }

    services
}

fn extract_timers(unit_dir: &Path) -> Vec<String> {
    let entries = match fs::read_dir(unit_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    entries
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".timer")
                .map(String::from)
        })
        .collect()
}

/// Parse the declarative users-groups.json from the system closure.
///
/// NixOS stores the full user/group specification as a JSON file in the nix
/// store, referenced from the activation script.
fn find_users_groups_json(root: &Path) -> Option<String> {
    let activate = fs::read_to_string(root.join("activate")).ok()?;
    // The activate script contains a reference like:
    //   /nix/store/xxx-users-groups.json
    activate
        .split_whitespace()
        .find(|s| s.contains("users-groups.json"))
        .map(String::from)
}

fn extract_users(root: &Path) -> BTreeMap<String, UserInfo> {
    let mut users = BTreeMap::new();

    let json_path = match find_users_groups_json(root) {
        Some(p) => p,
        None => {
            eprintln!("warning: cannot find users-groups.json in system closure");
            return users;
        }
    };

    let content = match fs::read_to_string(&json_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("warning: cannot read {json_path}: {e}");
            return users;
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("warning: cannot parse {json_path}: {e}");
            return users;
        }
    };

    if let Some(user_list) = parsed.get("users").and_then(|v| v.as_array()) {
        for user in user_list {
            let name = match user.get("name").and_then(|v| v.as_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            // Skip nixbld users
            if name.starts_with("nixbld") {
                continue;
            }

            let uid = user.get("uid").and_then(|v| v.as_u64()).map(|u| u as u32);
            let group = user
                .get("group")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let is_system_user = user
                .get("isSystemUser")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_normal_user = !is_system_user && uid.is_some_and(|u| u != 0);

            users.insert(
                name,
                UserInfo {
                    uid,
                    group,
                    is_system_user,
                    is_normal_user,
                },
            );
        }
    }

    users
}

fn extract_groups(root: &Path) -> Vec<String> {
    let json_path = match find_users_groups_json(root) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let content = match fs::read_to_string(&json_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    parsed
        .get("groups")
        .and_then(|v| v.as_array())
        .map(|groups| {
            groups
                .iter()
                .filter_map(|g| g.get("name").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract firewall ports from the declared firewall-start script in the store.
///
/// No iptables, no root — just reads the script that NixOS generated.
fn extract_firewall(unit_dir: &Path) -> FirewallInfo {
    let fw_unit = unit_dir.join("firewall.service");
    let enabled = fw_unit.exists();

    if !enabled {
        return FirewallInfo {
            enable: false,
            allowed_tcp_ports: Vec::new(),
            allowed_udp_ports: Vec::new(),
        };
    }

    let mut tcp_ports = Vec::new();
    let mut udp_ports = Vec::new();

    // Find the firewall-start script path from the unit file
    if let Ok(unit_content) = fs::read_to_string(&fw_unit)
        && let Some(start_script) = extract_exec_start(&unit_content, "firewall-start")
        && let Ok(script) = fs::read_to_string(format!("{start_script}/bin/firewall-start"))
    {
        parse_firewall_script(&script, &mut tcp_ports, &mut udp_ports);
    }

    tcp_ports.sort();
    tcp_ports.dedup();
    udp_ports.sort();
    udp_ports.dedup();

    FirewallInfo {
        enable: true,
        allowed_tcp_ports: tcp_ports,
        allowed_udp_ports: udp_ports,
    }
}

/// Extract a store path from an ExecStart= line matching a given binary name.
fn extract_exec_start(unit_content: &str, binary_name: &str) -> Option<String> {
    for line in unit_content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("ExecStart=") {
            // Format: @/nix/store/xxx-name/bin/name name
            // or:      /nix/store/xxx-name/bin/name
            let path = rest.trim_start_matches('@');
            if path.contains(binary_name) {
                // Extract the store path (up to /bin/...)
                if let Some(idx) = path.find("/bin/") {
                    return Some(path[..idx].to_string());
                }
            }
        }
    }
    None
}

/// Parse the NixOS-generated firewall script for port rules.
fn parse_firewall_script(script: &str, tcp_ports: &mut Vec<u16>, udp_ports: &mut Vec<u16>) {
    for line in script.lines() {
        let line = line.trim();
        // Match lines like: ip46tables -A nixos-fw -p tcp --dport 22 -j nixos-fw-accept
        if !line.contains("nixos-fw-accept") {
            continue;
        }
        if let Some(port) = extract_dport(line) {
            if line.contains("-p tcp") {
                tcp_ports.push(port);
            } else if line.contains("-p udp") {
                udp_ports.push(port);
            }
        }
    }
}

fn extract_dport(line: &str) -> Option<u16> {
    let mut parts = line.split_whitespace();
    while let Some(part) = parts.next() {
        if part == "--dport" {
            return parts.next()?.parse().ok();
        }
    }
    None
}

fn extract_nginx_vhosts(etc_dir: &Path) -> Vec<String> {
    // NixOS generates nginx config in the store
    let nginx_conf = etc_dir.join("nginx/nginx.conf");
    if let Ok(content) = fs::read_to_string(nginx_conf) {
        let mut vhosts: Vec<String> = content
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("server_name ")
                    .map(|rest| rest.trim_end_matches(';').trim().to_string())
            })
            .collect();
        vhosts.sort();
        vhosts.dedup();
        return vhosts;
    }

    Vec::new()
}

/// Get declared packages from the system path's direct store references.
///
/// Uses `nix-store --references` (not `-qR` which gives the transitive closure).
/// Direct references = the packages NixOS was told to include.
fn extract_packages(root: &Path) -> Vec<String> {
    let sw = root.join("sw");
    let output = match std::process::Command::new("nix-store")
        .args(["--query", "--references"])
        .arg(&sw)
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(_) => {
            eprintln!("warning: nix-store query failed");
            return Vec::new();
        }
        Err(e) => {
            eprintln!("warning: cannot run nix-store: {e}");
            return Vec::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            let path = line.strip_prefix("/nix/store/")?;
            // hash-name-version → name-version
            let (_, name) = path.split_once('-')?;
            Some(name.to_string())
        })
        .collect();

    packages.sort();
    packages.dedup();
    packages
}

fn extract_etc_files(etc_dir: &Path) -> Vec<String> {
    if !etc_dir.exists() {
        return Vec::new();
    }

    let mut files = Vec::new();
    collect_etc_files(etc_dir, "", &mut files);
    files.sort();
    files
}

fn collect_etc_files(dir: &Path, prefix: &str, files: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip NixOS metadata files (ownership/permissions)
        if name.ends_with(".gid") || name.ends_with(".uid") || name.ends_with(".mode") {
            continue;
        }

        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            collect_etc_files(&entry.path(), &path, files);
        } else {
            files.push(path);
        }
    }
}

fn extract_postgresql(unit_dir: &Path) -> PostgresqlInfo {
    let enabled = unit_dir.join("postgresql.service").exists();

    // PostgreSQL databases/users are declared in the ensure scripts,
    // but extracting them from the store scripts is fragile. For now,
    // just report whether postgresql is enabled.
    PostgresqlInfo {
        enable: enabled,
        ensure_databases: Vec::new(),
        ensure_users: Vec::new(),
    }
}
