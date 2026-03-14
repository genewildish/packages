//! Rust ecosystem — detects `Cargo.toml` and compares against `Cargo.lock`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::Ecosystem;
use crate::types::{PackageInfo, PackageStatus};

pub struct RustEcosystem;

impl Ecosystem for RustEcosystem {
    fn name(&self) -> &str {
        "Rust"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("Cargo.toml").exists()
    }

    fn check_packages(&self, project_dir: &Path) -> Result<Vec<PackageInfo>, String> {
        let cargo_toml = project_dir.join("Cargo.toml");
        let content = fs::read_to_string(&cargo_toml)
            .map_err(|e| format!("Failed to read {}: {}", cargo_toml.display(), e))?;
        let parsed: toml::Value = content
            .parse()
            .map_err(|e| format!("Failed to parse {}: {}", cargo_toml.display(), e))?;

        // Collect from [dependencies], [dev-dependencies], [build-dependencies].
        let mut declared: Vec<(String, String)> = Vec::new();
        for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(deps) = parsed.get(section).and_then(|v| v.as_table()) {
                for (name, val) in deps {
                    declared.push((name.clone(), extract_version(val)));
                }
            }
        }

        // Parse Cargo.lock for resolved versions.
        let locked = parse_cargo_lock(project_dir);

        let mut packages = Vec::new();
        for (name, required_ver) in declared {
            let (status, version) = match locked.get(&name) {
                Some(locked_ver) => (
                    PackageStatus::Installed {
                        version: locked_ver.clone(),
                    },
                    locked_ver.clone(),
                ),
                None => (PackageStatus::Missing, required_ver.clone()),
            };

            // In Rust source code, hyphens in crate names become underscores.
            let import_alias = name.replace('-', "_");

            packages.push(PackageInfo {
                name,
                description: "N/A".to_string(), // Cargo.lock has no descriptions.
                language: "Rust".to_string(),
                version,
                status,
                usage_percent: 0.0,
                import_aliases: vec![import_alias],
            });
        }

        // Fetch descriptions from crates.io for packages lacking one.
        backfill_descriptions(&mut packages);

        Ok(packages)
    }

    fn source_extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn import_patterns(&self) -> Vec<String> {
        vec![
            // use crate_name::...
            r"(?m)^\s*use\s+([a-zA-Z_][a-zA-Z0-9_]*)::".to_string(),
            // extern crate crate_name
            r"(?m)^\s*extern\s+crate\s+([a-zA-Z_][a-zA-Z0-9_]*)".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Registry descriptions
// ---------------------------------------------------------------------------

/// Fetch descriptions from crates.io for any package still showing "N/A".
fn backfill_descriptions(packages: &mut [PackageInfo]) {
    for pkg in packages.iter_mut() {
        if pkg.description == "N/A" {
            if let Some(desc) = fetch_crate_description(&pkg.name) {
                pkg.description = desc;
            }
        }
    }
}

/// Query the crates.io API for a crate's description.
/// Returns `None` on any network or parse error (graceful degradation).
fn fetch_crate_description(name: &str) -> Option<String> {
    let url = format!("https://crates.io/api/v1/crates/{}", name);
    let body: serde_json::Value = ureq::get(&url)
        .set("User-Agent", "pkgcheck/0.1.0")
        .call()
        .ok()?
        .into_json()
        .ok()?;
    body.get("crate")?
        .get("description")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract version string from a TOML dependency value, which may be a plain
/// string (`"1.0"`) or a table (`{ version = "1.0", features = [...] }`).
fn extract_version(val: &toml::Value) -> String {
    match val {
        toml::Value::String(s) => s.clone(),
        toml::Value::Table(t) => t
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("*")
            .to_string(),
        _ => "*".to_string(),
    }
}

/// Parse `Cargo.lock` and return a map of package_name → resolved version.
fn parse_cargo_lock(project_dir: &Path) -> HashMap<String, String> {
    let lock_path = project_dir.join("Cargo.lock");
    let mut map = HashMap::new();

    let Ok(content) = fs::read_to_string(&lock_path) else {
        return map;
    };
    let Ok(parsed) = content.parse::<toml::Value>() else {
        return map;
    };

    if let Some(packages) = parsed.get("package").and_then(|p| p.as_array()) {
        for pkg in packages {
            if let (Some(name), Some(version)) = (
                pkg.get("name").and_then(|n| n.as_str()),
                pkg.get("version").and_then(|v| v.as_str()),
            ) {
                map.insert(name.to_string(), version.to_string());
            }
        }
    }

    map
}
