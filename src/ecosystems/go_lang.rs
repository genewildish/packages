//! Go ecosystem — detects `go.mod` and checks module installation via `go list`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::Ecosystem;
use crate::types::{PackageInfo, PackageStatus};

pub struct GoEcosystem;

impl Ecosystem for GoEcosystem {
    fn name(&self) -> &str {
        "Go"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("go.mod").exists()
    }

    fn check_packages(&self, project_dir: &Path) -> Result<Vec<PackageInfo>, String> {
        let go_mod = project_dir.join("go.mod");
        let content = fs::read_to_string(&go_mod)
            .map_err(|e| format!("Failed to read {}: {}", go_mod.display(), e))?;

        let declared = parse_go_mod(&content);
        if declared.is_empty() {
            return Ok(Vec::new());
        }

        // Snapshot of downloaded modules via `go list -m all`.
        let installed = installed_go_modules(project_dir);

        let mut packages = Vec::new();
        for (module_path, required_ver) in declared {
            let (status, version) = match installed.get(&module_path) {
                Some(ver) => (
                    PackageStatus::Installed {
                        version: ver.clone(),
                    },
                    ver.clone(),
                ),
                None => (PackageStatus::Missing, required_ver.clone()),
            };

            // Use the last path segment as the display name.
            let short_name = module_path
                .rsplit('/')
                .next()
                .unwrap_or(&module_path)
                .to_string();

            packages.push(PackageInfo {
                name: short_name,
                // Show the full module path as the description since there is
                // no local source of one-line descriptions for Go modules.
                description: module_path.clone(),
                language: "Go".to_string(),
                version,
                status,
                usage_percent: 0.0,
                // Go imports use the full module path (or a sub-path of it).
                import_aliases: vec![module_path],
            });
        }

        Ok(packages)
    }

    fn source_extensions(&self) -> &[&str] {
        &["go"]
    }

    fn import_patterns(&self) -> Vec<String> {
        vec![
            // Single-line import:  import "path"
            r#"(?m)^\s*import\s+"([^"]+)""#.to_string(),
            // Inside an import block:  "path"
            r#"(?m)^\s+"([^"]+)""#.to_string(),
            // Aliased import inside block:  alias "path"
            r#"(?m)^\s+\S+\s+"([^"]+)""#.to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// go.mod parsing
// ---------------------------------------------------------------------------

/// Extract `require` directives from a `go.mod` file.
fn parse_go_mod(content: &str) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    let mut in_require_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("require (") || trimmed == "require (" {
            in_require_block = true;
            continue;
        }

        if in_require_block {
            if trimmed == ")" {
                in_require_block = false;
                continue;
            }
            if let Some(pair) = parse_require_line(trimmed) {
                deps.push(pair);
            }
        } else if let Some(rest) = trimmed.strip_prefix("require ") {
            // Single-line require.
            if let Some(pair) = parse_require_line(rest) {
                deps.push(pair);
            }
        }
    }

    deps
}

/// Parse a single require line like `github.com/foo/bar v1.2.3 // indirect`.
fn parse_require_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") {
        return None;
    }
    // Strip trailing `// indirect` comments.
    let clean = trimmed.split("//").next().unwrap_or(trimmed).trim();
    let parts: Vec<&str> = clean.split_whitespace().collect();
    if parts.len() >= 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Installed module inspection
// ---------------------------------------------------------------------------

/// Run `go list -m all` in the project directory and return a map of
/// module_path → version.
fn installed_go_modules(project_dir: &Path) -> HashMap<String, String> {
    let output = Command::new("go")
        .args(["list", "-m", "all"])
        .current_dir(project_dir)
        .output();

    let Ok(output) = output else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            map.insert(parts[0].to_string(), parts[1].to_string());
        }
    }
    map
}
