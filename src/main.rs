//! pkgcheck — check whether all packages required by the current project
//! are installed.
//!
//! Run from a project root that contains one or more supported manifest files
//! (package.json, requirements.txt, Cargo.toml, go.mod, Gemfile, …).

mod detect;
mod display;
mod ecosystems;
mod scanner;
mod types;

use std::process;

use display::Display;
use types::{EcosystemSummary, OverallStatus, PackageStatus};

fn main() {
    if let Err(msg) = run() {
        eprintln!("Error: {}", msg);
        process::exit(1);
    }
}

/// Top-level orchestration.  Returns `Err` only for truly fatal problems
/// (e.g. cannot determine CWD); per-ecosystem errors are reported inline
/// and never halt the run.
fn run() -> Result<(), String> {
    // Determine the project directory (current working directory).
    let project_dir = std::env::current_dir()
        .map_err(|e| format!("Could not determine current directory: {}", e))?;

    // Detect which language ecosystems are present.
    let ecosystems = detect::detect_ecosystems(&project_dir);

    if ecosystems.is_empty() {
        println!(
            "No package manifests detected in {}",
            project_dir.display()
        );
        return Ok(());
    }

    // Start the live status display (spawns a blink thread).
    let mut display = Display::new();

    // Check each ecosystem's packages and scan import usage.
    let mut all_packages = Vec::new();

    for eco in &ecosystems {
        display.start_ecosystem(eco.name());

        match eco.check_packages(&project_dir) {
            Ok(mut packages) => {
                // Compute what fraction of source files imports each package.
                scanner::compute_usage(
                    &project_dir,
                    &mut packages,
                    eco.source_extensions(),
                    &eco.import_patterns(),
                );

                let summary = EcosystemSummary::from_packages(eco.name(), &packages);
                display.finish_ecosystem(summary);
                all_packages.extend(packages);
            }
            Err(msg) => {
                // Report the error but keep going with remaining ecosystems.
                eprintln!("  Warning: {}: {}", eco.name(), msg);
                display.finish_ecosystem(EcosystemSummary {
                    name: eco.name().to_string(),
                    total: 0,
                    installed: 0,
                    outdated: 0,
                    missing: 0,
                });
            }
        }
    }

    // Compute the overall health indicator.
    let status = compute_overall_status(&all_packages);
    display.set_final_status(status);

    // Stop the blink thread and render the final status.
    display.finish();

    // Print the summary table.
    display.print_table(&all_packages);

    Ok(())
}

/// Derive the overall project health from the aggregated package list.
fn compute_overall_status(packages: &[types::PackageInfo]) -> OverallStatus {
    if packages.is_empty() {
        return OverallStatus::AllGood;
    }

    let total = packages.len();
    let installed = packages
        .iter()
        .filter(|p| matches!(p.status, PackageStatus::Installed { .. }))
        .count();
    let outdated = packages
        .iter()
        .filter(|p| matches!(p.status, PackageStatus::OutOfDate { .. }))
        .count();

    if installed + outdated == total {
        if outdated > 0 {
            OverallStatus::Partial
        } else {
            OverallStatus::AllGood
        }
    } else if installed == 0 && outdated == 0 {
        OverallStatus::NoneInstalled
    } else {
        OverallStatus::Partial
    }
}
