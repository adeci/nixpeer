use crate::diff::{ChangeEntry, ChangeSection};
use crate::extract::SystemSummary;
use owo_colors::OwoColorize;
use serde::Serialize;

/// Print all change sections with colored output.
pub fn print_changes(before_ref: &str, after_ref: &str, sections: &[ChangeSection]) {
    let total: usize = sections.iter().map(|s| s.entries.len()).sum();
    let section_count = sections.len();

    println!();
    println!(
        "  {} → {}  {}",
        before_ref.dimmed(),
        after_ref.bold(),
        format!("({total} changes across {section_count} sections)").dimmed(),
    );
    println!();

    for section in sections {
        print_section(section);
    }
}

fn print_section(section: &ChangeSection) {
    println!("  {}", section.name.bold().underline());
    println!();

    for entry in &section.entries {
        match entry {
            ChangeEntry::Added(name, detail) => {
                print!("    {} {}", "+".green().bold(), name.green());
                if let Some(d) = detail {
                    print!("  {}", d.dimmed());
                }
                println!();
            }
            ChangeEntry::Removed(name, detail) => {
                print!("    {} {}", "-".red().bold(), name.red());
                if let Some(d) = detail {
                    print!("  {}", d.dimmed());
                }
                println!();
            }
            ChangeEntry::Modified(name, desc) => {
                println!(
                    "    {} {}  {}",
                    "~".yellow().bold(),
                    name.yellow(),
                    desc.dimmed()
                );
            }
        }
    }

    println!();
}

// --- JSON export ---

#[derive(Serialize)]
struct JsonReport<'a> {
    before: &'a str,
    after: &'a str,
    total_changes: usize,
    sections: Vec<JsonSection<'a>>,
}

#[derive(Serialize)]
struct JsonSection<'a> {
    name: &'a str,
    changes: Vec<JsonChange>,
}

#[derive(Serialize)]
struct JsonChange {
    kind: &'static str,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

/// Serialize changes as JSON.
pub fn json_changes(
    before: &SystemSummary,
    after: &SystemSummary,
    sections: &[ChangeSection],
) -> String {
    let before_label = before.machine.label();
    let after_label = after.machine.label();

    let report = JsonReport {
        before: if before_label.is_empty() {
            "before"
        } else {
            &before_label
        },
        after: if after_label.is_empty() {
            "after"
        } else {
            &after_label
        },
        total_changes: sections.iter().map(|s| s.entries.len()).sum(),
        sections: sections
            .iter()
            .map(|s| JsonSection {
                name: s.name,
                changes: s
                    .entries
                    .iter()
                    .map(|e| match e {
                        ChangeEntry::Added(name, detail) => JsonChange {
                            kind: "added",
                            name: name.clone(),
                            detail: detail.clone(),
                        },
                        ChangeEntry::Removed(name, detail) => JsonChange {
                            kind: "removed",
                            name: name.clone(),
                            detail: detail.clone(),
                        },
                        ChangeEntry::Modified(name, desc) => JsonChange {
                            kind: "modified",
                            name: name.clone(),
                            detail: Some(desc.clone()),
                        },
                    })
                    .collect(),
            })
            .collect(),
    };

    serde_json::to_string_pretty(&report).expect("failed to serialize JSON report")
}
