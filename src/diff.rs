use crate::extract::{FirewallInfo, ServiceInfo, SystemSummary, UserInfo};
use std::collections::{BTreeMap, BTreeSet};

/// A single section of changes (e.g., "systemd services", "users").
pub struct ChangeSection {
    pub name: &'static str,
    pub entries: Vec<ChangeEntry>,
}

pub enum ChangeEntry {
    Added(String, Option<String>),
    Removed(String, Option<String>),
    Modified(String, String),
}

/// Diff two system summaries and return all sections that have changes.
pub fn diff(before: &SystemSummary, after: &SystemSummary) -> Vec<ChangeSection> {
    let candidates = [
        diff_services(&before.systemd_services, &after.systemd_services),
        diff_lists(
            "systemd timers",
            &before.systemd_timers,
            &after.systemd_timers,
        ),
        diff_users(&before.users, &after.users),
        diff_lists("groups", &before.groups, &after.groups),
        diff_firewall(&before.firewall, &after.firewall),
        diff_lists(
            "environment packages",
            &before.environment_packages,
            &after.environment_packages,
        ),
        diff_etc_files(&before.etc_files, &after.etc_files),
    ];

    candidates
        .into_iter()
        .filter(|s| !s.entries.is_empty())
        .collect()
}

fn diff_services(
    before: &BTreeMap<String, ServiceInfo>,
    after: &BTreeMap<String, ServiceInfo>,
) -> ChangeSection {
    let mut entries = Vec::new();

    for (name, info) in after {
        match before.get(name) {
            None => {
                let detail = if info.description.is_empty() {
                    None
                } else {
                    Some(info.description.clone())
                };
                entries.push(ChangeEntry::Added(name.clone(), detail));
            }
            Some(old) => {
                if old.description != info.description && !info.description.is_empty() {
                    entries.push(ChangeEntry::Modified(
                        name.clone(),
                        format!("\"{}\" → \"{}\"", old.description, info.description),
                    ));
                }
            }
        }
    }

    for name in before.keys() {
        if !after.contains_key(name) {
            entries.push(ChangeEntry::Removed(name.clone(), None));
        }
    }

    ChangeSection {
        name: "systemd services",
        entries,
    }
}

fn diff_users(
    before: &BTreeMap<String, UserInfo>,
    after: &BTreeMap<String, UserInfo>,
) -> ChangeSection {
    let mut entries = Vec::new();

    for (name, info) in after {
        let detail = || {
            let kind = if info.is_normal_user {
                "normal"
            } else if info.is_system_user {
                "system"
            } else {
                "service"
            };
            match info.uid {
                Some(uid) => format!("{kind}, uid={uid}, group={}", info.group),
                None => format!("{kind}, group={}", info.group),
            }
        };

        match before.get(name) {
            None => entries.push(ChangeEntry::Added(name.clone(), Some(detail()))),
            Some(old) => {
                let mut changes = Vec::new();
                if old.uid != info.uid {
                    changes.push(format!(
                        "uid: {} → {}",
                        old.uid.map_or("none".into(), |u| u.to_string()),
                        info.uid.map_or("none".into(), |u| u.to_string()),
                    ));
                }
                if old.group != info.group {
                    changes.push(format!("group: {} → {}", old.group, info.group));
                }
                if old.is_system_user != info.is_system_user
                    || old.is_normal_user != info.is_normal_user
                {
                    changes.push("user type changed".into());
                }
                if !changes.is_empty() {
                    entries.push(ChangeEntry::Modified(name.clone(), changes.join(", ")));
                }
            }
        }
    }

    for name in before.keys() {
        if !after.contains_key(name) {
            entries.push(ChangeEntry::Removed(name.clone(), None));
        }
    }

    ChangeSection {
        name: "users",
        entries,
    }
}

fn diff_lists(name: &'static str, before: &[String], after: &[String]) -> ChangeSection {
    let before_set: BTreeSet<_> = before.iter().collect();
    let after_set: BTreeSet<_> = after.iter().collect();

    let mut entries = Vec::new();

    for item in &after_set {
        if !before_set.contains(item) {
            entries.push(ChangeEntry::Added((**item).clone(), None));
        }
    }

    for item in &before_set {
        if !after_set.contains(item) {
            entries.push(ChangeEntry::Removed((**item).clone(), None));
        }
    }

    ChangeSection { name, entries }
}

fn diff_firewall(before: &FirewallInfo, after: &FirewallInfo) -> ChangeSection {
    let mut entries = Vec::new();

    if before.enable != after.enable {
        let state = |v: bool| if v { "enabled" } else { "disabled" };
        entries.push(ChangeEntry::Modified(
            "firewall".to_string(),
            format!("{} → {}", state(before.enable), state(after.enable)),
        ));
    }

    diff_port_set(
        &mut entries,
        "tcp",
        &before.allowed_tcp_ports,
        &after.allowed_tcp_ports,
    );
    diff_port_set(
        &mut entries,
        "udp",
        &before.allowed_udp_ports,
        &after.allowed_udp_ports,
    );

    ChangeSection {
        name: "firewall",
        entries,
    }
}

fn diff_port_set(entries: &mut Vec<ChangeEntry>, proto: &str, before: &[u16], after: &[u16]) {
    let before_set: BTreeSet<_> = before.iter().collect();
    let after_set: BTreeSet<_> = after.iter().collect();

    for port in &after_set {
        if !before_set.contains(port) {
            entries.push(ChangeEntry::Added(format!("{proto}/{port}"), None));
        }
    }
    for port in &before_set {
        if !after_set.contains(port) {
            entries.push(ChangeEntry::Removed(format!("{proto}/{port}"), None));
        }
    }
}

/// Diff etc files with modification detection.
///
/// Etc entries are symlinks into the nix store. Same path with different
/// store targets means the file contents changed.
fn diff_etc_files(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> ChangeSection {
    let mut entries = Vec::new();

    for (path, target) in after {
        match before.get(path) {
            None => entries.push(ChangeEntry::Added(path.clone(), None)),
            Some(old_target) => {
                if old_target != target {
                    entries.push(ChangeEntry::Modified(path.clone(), "contents changed".into()));
                }
            }
        }
    }

    for path in before.keys() {
        if !after.contains_key(path) {
            entries.push(ChangeEntry::Removed(path.clone(), None));
        }
    }

    ChangeSection {
        name: "etc files",
        entries,
    }
}
