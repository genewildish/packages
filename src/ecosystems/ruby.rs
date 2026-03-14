//! Ruby ecosystem — detects `Gemfile` and checks `Gemfile.lock`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::Ecosystem;
use crate::types::{PackageInfo, PackageStatus};

pub struct RubyEcosystem;

impl Ecosystem for RubyEcosystem {
    fn name(&self) -> &str {
        "Ruby"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("Gemfile").exists()
    }

    fn check_packages(&self, project_dir: &Path) -> Result<Vec<PackageInfo>, String> {
        let gemfile = project_dir.join("Gemfile");
        let content = fs::read_to_string(&gemfile)
            .map_err(|e| format!("Failed to read {}: {}", gemfile.display(), e))?;

        let declared = parse_gemfile(&content);
        if declared.is_empty() {
            return Ok(Vec::new());
        }

        // Parse Gemfile.lock for resolved versions.
        let locked = parse_gemfile_lock(project_dir);

        let mut packages = Vec::new();
        for (name, required_ver) in declared {
            let (status, version) = match locked.get(&name) {
                Some(ver) => (
                    PackageStatus::Installed {
                        version: ver.clone(),
                    },
                    ver.clone(),
                ),
                None => (PackageStatus::Missing, required_ver.clone()),
            };

            packages.push(PackageInfo {
                name: name.clone(),
                description: "N/A".to_string(), // No local description source.
                language: "Ruby".to_string(),
                version,
                status,
                usage_percent: 0.0,
                import_aliases: vec![name],
            });
        }

        Ok(packages)
    }

    fn source_extensions(&self) -> &[&str] {
        &["rb"]
    }

    fn import_patterns(&self) -> Vec<String> {
        vec![
            // require 'gem_name'  or  require "gem_name"
            // Excludes relative paths starting with . or /
            r#"(?m)^\s*require[\s(]+['"]([^'"./][^'"]*)['"]"#.to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Gemfile parsing
// ---------------------------------------------------------------------------

/// Extract `gem 'name', 'version'` lines from a Gemfile.
fn parse_gemfile(content: &str) -> Vec<(String, String)> {
    let mut deps = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("gem ") {
            let rest = rest.trim();
            if let Some(name) = extract_quoted(rest) {
                // Attempt to pull a version from the second argument.
                let version = rest
                    .splitn(2, ',')
                    .nth(1)
                    .and_then(|v| extract_quoted(v.trim()))
                    .unwrap_or_else(|| "*".to_string());
                deps.push((name, version));
            }
        }
    }
    deps
}

/// Return the content of the first single- or double-quoted string in `s`.
fn extract_quoted(s: &str) -> Option<String> {
    let quote = s.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let end = s[1..].find(quote)?;
    Some(s[1..1 + end].to_string())
}

// ---------------------------------------------------------------------------
// Gemfile.lock parsing
// ---------------------------------------------------------------------------

/// Parse the GEM/specs section of `Gemfile.lock` for resolved gem versions.
fn parse_gemfile_lock(project_dir: &Path) -> HashMap<String, String> {
    let lock_path = project_dir.join("Gemfile.lock");
    let mut map = HashMap::new();

    let Ok(content) = fs::read_to_string(&lock_path) else {
        return map;
    };

    let mut in_specs = false;
    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "specs:" {
            in_specs = true;
            continue;
        }

        // A non-indented, non-empty line ends the specs section.
        if in_specs && !line.starts_with(' ') && !trimmed.is_empty() {
            in_specs = false;
            continue;
        }

        if in_specs {
            // Top-level spec lines have exactly 4 spaces of indent:
            //     gem_name (1.2.3)
            let indent = line.len() - line.trim_start().len();
            if indent == 4 {
                if let Some((name, ver)) = parse_spec_line(trimmed) {
                    map.insert(name, ver);
                }
            }
        }
    }

    map
}

/// Parse a spec line like `rails (7.0.4)` into `(name, version)`.
fn parse_spec_line(line: &str) -> Option<(String, String)> {
    let paren_start = line.find('(')?;
    let paren_end = line.find(')')?;
    let name = line[..paren_start].trim().to_string();
    let version = line[paren_start + 1..paren_end].to_string();
    Some((name, version))
}
