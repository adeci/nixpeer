mod diff;
mod display;
mod extract;
mod live;
mod peer;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process;

/// Compare NixOS systems, locally or peer-to-peer.
///
/// Extract system-level artifacts (services, users, ports, packages) from NixOS
/// configurations and diff them — either between two local flake refs or across
/// the network with another NixOS user via iroh P2P.
///
/// When a flake ref is omitted, nixdelta extracts from the currently running
/// NixOS system — no source code needed.
#[derive(Parser)]
#[command(name = "nixdelta", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Path to the extractor Nix expression (only needed for flake refs).
    #[arg(long, global = true)]
    extractor: Option<PathBuf>,

    /// Output results as JSON instead of colored text.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Compare two local NixOS configurations.
    Diff {
        /// Flake reference for the "before" configuration.
        before: String,
        /// Flake reference for the "after" configuration.
        after: String,
    },

    /// Share your system over P2P and wait for a peer to compare.
    ///
    /// If no flake ref is given, shares the currently running NixOS system.
    Share {
        /// Flake reference (omit to use the running system).
        flake_ref: Option<String>,
    },

    /// Compare your system against a peer's shared summary.
    ///
    /// If no flake ref is given, compares the currently running NixOS system.
    Compare {
        /// Connection ticket from the peer's `share` command.
        ticket: String,
        /// Flake reference (omit to use the running system).
        flake_ref: Option<String>,
    },

    /// Compare two NixOS generations on this machine.
    ///
    /// Shows what changed between system generations (e.g. after nixos-rebuild).
    /// If only one generation is given, compares it against the current system.
    Generations {
        /// First generation number.
        before: u64,
        /// Second generation number (omit to compare against current system).
        after: Option<u64>,
    },
}

fn resolve_extractor(cli_path: &Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_path {
        return p.clone();
    }
    if let Ok(exe) = std::env::current_exe() {
        let beside_exe = exe.parent().unwrap().join("extract.nix");
        if beside_exe.exists() {
            return beside_exe;
        }
    }
    let cwd = PathBuf::from("extract.nix");
    if cwd.exists() {
        return cwd;
    }
    eprintln!("error: could not find extract.nix — pass --extractor <path>");
    process::exit(1);
}

/// Extract a system summary from either a flake ref or the running system.
fn get_summary(flake_ref: &Option<String>, extractor: &Option<PathBuf>) -> extract::SystemSummary {
    match flake_ref {
        Some(fref) => {
            let extractor_path = resolve_extractor(extractor);
            eprintln!("extracting: {fref}");
            match extract::extract(fref, &extractor_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            }
        }
        None => {
            eprintln!("extracting from running system...");
            match live::extract_live() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

fn show_diff(
    before: &extract::SystemSummary,
    after: &extract::SystemSummary,
    changes: &[diff::ChangeSection],
    json: bool,
) {
    if json {
        let output = display::json_changes(before, after, changes);
        println!("{output}");
    } else if changes.is_empty() {
        eprintln!("no changes detected");
    } else {
        let before_label = before.machine.label();
        let after_label = after.machine.label();
        let before_label = if before_label.is_empty() {
            "before".into()
        } else {
            before_label
        };
        let after_label = if after_label.is_empty() {
            "after".into()
        } else {
            after_label
        };
        display::print_changes(&before_label, &after_label, changes);
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Diff { before, after } => {
            let before_summary = get_summary(&Some(before.clone()), &cli.extractor);
            let after_summary = get_summary(&Some(after.clone()), &cli.extractor);
            let changes = diff::diff(&before_summary, &after_summary);
            show_diff(&before_summary, &after_summary, &changes, cli.json);
        }

        Command::Share { flake_ref } => {
            let summary = get_summary(&flake_ref, &cli.extractor);
            let json = serde_json::to_vec(&summary).expect("failed to serialize summary");

            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            let peer_json = match rt.block_on(peer::share(json)) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };

            let peer_summary: extract::SystemSummary =
                serde_json::from_slice(&peer_json).expect("failed to parse peer summary");

            let changes = diff::diff(&summary, &peer_summary);
            show_diff(&summary, &peer_summary, &changes, cli.json);
        }

        Command::Generations { before, after } => {
            let before_summary = match live::extract_generation(before) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };

            let after_summary = match after {
                Some(g) => match live::extract_generation(g) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: {e}");
                        process::exit(1);
                    }
                },
                None => match live::extract_live() {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: {e}");
                        process::exit(1);
                    }
                },
            };

            let before_label = format!("gen {before}");
            let after_label = after.map_or("current".into(), |g| format!("gen {g}"));
            eprintln!("{before_label} → {after_label}");

            let changes = diff::diff(&before_summary, &after_summary);
            show_diff(&before_summary, &after_summary, &changes, cli.json);
        }

        Command::Compare { ticket, flake_ref } => {
            let my_summary = get_summary(&flake_ref, &cli.extractor);
            let my_json = serde_json::to_vec(&my_summary).expect("failed to serialize summary");

            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            let peer_json = match rt.block_on(peer::compare(my_json, &ticket)) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };

            let peer_summary: extract::SystemSummary =
                serde_json::from_slice(&peer_json).expect("failed to parse peer summary");

            let changes = diff::diff(&peer_summary, &my_summary);
            show_diff(&peer_summary, &my_summary, &changes, cli.json);
        }
    }
}
