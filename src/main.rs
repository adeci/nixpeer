mod diff;
mod display;
mod extract;
mod live;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process;

/// Preview and review NixOS system changes.
///
/// Reads system-level declarations directly from the nix store.
#[derive(Parser)]
#[command(name = "nixdelta", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output results as JSON instead of colored text.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Preview what a rebuild would change.
    ///
    /// Compares the current running system against a target. The target can be
    /// a path to a built system closure or a flake ref.
    ///
    ///   nixos-rebuild build && nixdelta preview
    ///   nixdelta preview /path/to/result
    ///   nixdelta preview .#nixosConfigurations.myhost
    Preview {
        /// System closure path or flake ref (default: ./result).
        target: Option<String>,
    },

    /// Show what the last rebuild changed.
    ///
    /// Compares the previous generation against the current one.
    Report,

    /// Compare two NixOS generations.
    ///
    /// If only one generation is given, compares it against the current system.
    Generations {
        /// First generation number.
        before: u64,
        /// Second generation number (omit to compare against current system).
        after: Option<u64>,
    },

    /// Compare two NixOS configurations.
    ///
    /// Accepts flake refs or paths to system closures.
    Diff {
        /// First configuration (flake ref or store path).
        before: String,
        /// Second configuration (flake ref or store path).
        after: String,
    },
}

/// Check if a string looks like a flake ref (contains #).
fn is_flake_ref(s: &str) -> bool {
    s.contains('#')
}

/// Build a flake ref's toplevel system closure and return the store path.
fn build_flake(flake_ref: &str) -> PathBuf {
    let toplevel = format!("{flake_ref}.config.system.build.toplevel");
    eprintln!("building: {flake_ref}");
    let output = match std::process::Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths", &toplevel])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: failed to run nix build: {e}");
            process::exit(1);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("error: nix build failed:\n{stderr}");
        process::exit(1);
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    PathBuf::from(path)
}

/// Resolve a target string to a store path, building if it's a flake ref.
fn resolve_target(target: &str) -> PathBuf {
    if is_flake_ref(target) {
        build_flake(target)
    } else {
        let p = PathBuf::from(target);
        if !p.exists() {
            eprintln!("error: {target} not found");
            process::exit(1);
        }
        p
    }
}

fn extract_or_exit(path: &std::path::Path) -> extract::SystemSummary {
    match live::extract_system(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn show_diff(
    before: &extract::SystemSummary,
    after: &extract::SystemSummary,
    before_label: &str,
    after_label: &str,
    json: bool,
) {
    let changes = diff::diff(before, after);

    if json {
        let output = display::json_changes(before_label, after_label, &changes);
        println!("{output}");
    } else if changes.is_empty() {
        eprintln!("no changes");
    } else {
        display::print_changes(before_label, after_label, &changes);
    }
}

fn label_or(summary: &extract::SystemSummary, fallback: &str) -> String {
    let label = summary.machine.label();
    if label.is_empty() {
        fallback.into()
    } else {
        label
    }
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Preview { target } => {
            let target_str = target.unwrap_or_else(|| "./result".into());

            if !is_flake_ref(&target_str) && !PathBuf::from(&target_str).exists() {
                eprintln!("error: {target_str} not found");
                eprintln!();
                eprintln!("  build first:  nixos-rebuild build");
                eprintln!("  or specify:   nixdelta preview /path/to/result");
                eprintln!("  or a flake:   nixdelta preview .#nixosConfigurations.myhost");
                process::exit(1);
            }

            let target_path = resolve_target(&target_str);

            let current = match live::extract_live() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };
            let pending = extract_or_exit(&target_path);

            let current_label = label_or(&current, "current");
            let pending_label = label_or(&pending, "pending");
            show_diff(
                &current,
                &pending,
                &current_label,
                &format!("{pending_label} (pending)"),
                cli.json,
            );
        }

        Command::Report => {
            let current_gen = match live::current_generation() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };

            let generations = match live::list_generations() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };

            let previous_gen = generations
                .iter()
                .rev()
                .find(|&&g| g < current_gen)
                .copied();

            let Some(previous_gen) = previous_gen else {
                eprintln!("no previous generation to compare against");
                process::exit(0);
            };

            let before = match live::extract_generation(previous_gen) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };

            let after = match live::extract_generation(current_gen) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            };

            show_diff(
                &before,
                &after,
                &format!("gen {previous_gen}"),
                &format!("gen {current_gen} (current)"),
                cli.json,
            );
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

            show_diff(
                &before_summary,
                &after_summary,
                &format!("gen {before}"),
                &after.map_or("current".into(), |g| format!("gen {g}")),
                cli.json,
            );
        }

        Command::Diff {
            before: before_ref,
            after: after_ref,
        } => {
            let before_path = resolve_target(&before_ref);
            let after_path = resolve_target(&after_ref);

            let before = extract_or_exit(&before_path);
            let after = extract_or_exit(&after_path);

            show_diff(
                &before,
                &after,
                &label_or(&before, &before_ref),
                &label_or(&after, &after_ref),
                cli.json,
            );
        }
    }
}
