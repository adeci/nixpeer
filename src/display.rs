use crate::diff::{ChangeEntry, ChangeSection};
use owo_colors::OwoColorize;

/// Print all change sections with colored output.
pub fn print_changes(before_ref: &str, after_ref: &str, sections: &[ChangeSection]) {
    // Summary line
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
