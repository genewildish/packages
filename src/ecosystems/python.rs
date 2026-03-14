//! Python ecosystem — detects `requirements.txt`, `pyproject.toml`, and `Pipfile`.
//!
//! Installed-package inspection relies on `pip list` and `pip show`, both of
//! which respect the active virtual-environment automatically.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::Ecosystem;
use crate::types::{PackageInfo, PackageStatus};

pub struct PythonEcosystem;

impl Ecosystem for PythonEcosystem {
    fn name(&self) -> &str {
        "Python"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("requirements.txt").exists()
            || project_dir.join("pyproject.toml").exists()
            || project_dir.join("Pipfile").exists()
    }

    fn check_packages(&self, project_dir: &Path) -> Result<Vec<PackageInfo>, String> {
        // Gather declared dependency names + version specifiers.
        let declared = parse_declared_deps(project_dir)?;
        if declared.is_empty() {
            return Ok(Vec::new());
        }

        // Snapshot of all installed packages via `pip list --format=json`.
        let installed = installed_pip_packages();

        // Batch-fetch descriptions with a single `pip show` invocation.
        let pkg_names: Vec<&str> = declared.iter().map(|(n, _)| n.as_str()).collect();
        let descriptions = batch_pip_descriptions(&pkg_names);

        let mut packages = Vec::new();
        for (name, required_ver) in &declared {
            let normalized = normalize_name(name);

            let (status, version) = match installed.get(&normalized) {
                Some(inst_ver) => (
                    PackageStatus::Installed {
                        version: inst_ver.clone(),
                    },
                    inst_ver.clone(),
                ),
                None => (PackageStatus::Missing, required_ver.clone()),
            };

            let description = descriptions
                .get(&normalized)
                .cloned()
                .unwrap_or_else(|| "N/A".to_string());

            // Python import names replace hyphens with underscores.
            // NOTE: Some packages use entirely different import names
            // (e.g. Pillow → PIL). A comprehensive mapping is out of scope.
            let import_alias = name.to_lowercase().replace('-', "_");

            packages.push(PackageInfo {
                name: name.clone(),
                description,
                language: "Python".to_string(),
                version,
                status,
                usage_percent: 0.0,
                import_aliases: vec![import_alias],
            });
        }

        // Fetch descriptions from PyPI for packages still showing "N/A".
        backfill_descriptions(&mut packages);

        Ok(packages)
    }

    fn source_extensions(&self) -> &[&str] {
        &["py"]
    }

    fn import_patterns(&self) -> Vec<String> {
        vec![
            // import pkg   or   import pkg.sub
            r"(?m)^\s*import\s+([a-zA-Z_][a-zA-Z0-9_]*)".to_string(),
            // from pkg import ...   or   from pkg.sub import ...
            r"(?m)^\s*from\s+([a-zA-Z_][a-zA-Z0-9_]*)".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Registry descriptions
// ---------------------------------------------------------------------------

/// Fetch descriptions from PyPI for any package still showing "N/A".
fn backfill_descriptions(packages: &mut [PackageInfo]) {
    for pkg in packages.iter_mut() {
        if pkg.description == "N/A" {
            if let Some(desc) = fetch_pypi_description(&pkg.name) {
                pkg.description = desc;
            }
        }
    }
}

/// Query the PyPI JSON API for a package's summary.
fn fetch_pypi_description(name: &str) -> Option<String> {
    let url = format!("https://pypi.org/pypi/{}/json", name);
    let body: serde_json::Value = ureq::get(&url)
        .call()
        .ok()?
        .into_json()
        .ok()?;
    body.get("info")?
        .get("summary")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Manifest parsing
// ---------------------------------------------------------------------------

/// Collect declared dependency (name, version-specifier) pairs from whichever
/// manifest file is present (checked in priority order).
fn parse_declared_deps(project_dir: &Path) -> Result<Vec<(String, String)>, String> {
    let req_txt = project_dir.join("requirements.txt");
    if req_txt.exists() {
        return parse_requirements_txt(&req_txt);
    }

    let pyproject = project_dir.join("pyproject.toml");
    if pyproject.exists() {
        return parse_pyproject_toml(&pyproject);
    }

    let pipfile = project_dir.join("Pipfile");
    if pipfile.exists() {
        return parse_pipfile(&pipfile);
    }

    Ok(Vec::new())
}

/// Parse `requirements.txt` lines (e.g. `flask==2.3.0`, `requests>=2.28`).
fn parse_requirements_txt(path: &Path) -> Result<Vec<(String, String)>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    let mut deps = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // Skip blanks, comments, and pip option flags.
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        let name_end = trimmed
            .find(|c: char| c == '=' || c == '>' || c == '<' || c == '!' || c == ';' || c == '[')
            .unwrap_or(trimmed.len());
        let name = trimmed[..name_end].trim().to_string();
        let version = trimmed[name_end..].trim().to_string();
        if !name.is_empty() {
            deps.push((
                name,
                if version.is_empty() {
                    "*".to_string()
                } else {
                    version
                },
            ));
        }
    }
    Ok(deps)
}

/// Parse PEP 621 `[project] dependencies` from `pyproject.toml`.
fn parse_pyproject_toml(path: &Path) -> Result<Vec<(String, String)>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let parsed: toml::Value = content
        .parse()
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;

    let mut deps = Vec::new();

    if let Some(arr) = parsed
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
    {
        for item in arr {
            if let Some(spec) = item.as_str() {
                let name_end = spec
                    .find(|c: char| {
                        c == '=' || c == '>' || c == '<' || c == '!' || c == ';' || c == '['
                    })
                    .unwrap_or(spec.len());
                let name = spec[..name_end].trim().to_string();
                let version = spec[name_end..].trim().to_string();
                if !name.is_empty() {
                    deps.push((
                        name,
                        if version.is_empty() {
                            "*".to_string()
                        } else {
                            version
                        },
                    ));
                }
            }
        }
    }

    Ok(deps)
}

/// Minimal Pipfile parsing — reads `[packages]` keys.
fn parse_pipfile(path: &Path) -> Result<Vec<(String, String)>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let parsed: toml::Value = content
        .parse()
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;

    let mut deps = Vec::new();
    if let Some(pkgs) = parsed.get("packages").and_then(|p| p.as_table()) {
        for (name, val) in pkgs {
            let ver = match val.as_str() {
                Some(v) => v.to_string(),
                None => "*".to_string(),
            };
            deps.push((name.clone(), ver));
        }
    }
    Ok(deps)
}

// ---------------------------------------------------------------------------
// Installed-package inspection
// ---------------------------------------------------------------------------

/// Run `pip list --format=json` and return a map of normalised_name → version.
fn installed_pip_packages() -> HashMap<String, String> {
    let output = Command::new("pip").args(["list", "--format=json"]).output();

    let Ok(output) = output else {
        // pip not available; all packages will show as missing.
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return HashMap::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    for entry in entries {
        if let (Some(name), Some(ver)) = (
            entry.get("name").and_then(|n| n.as_str()),
            entry.get("version").and_then(|v| v.as_str()),
        ) {
            map.insert(normalize_name(name), ver.to_string());
        }
    }
    map
}

/// Fetch descriptions for multiple packages in a single `pip show` call.
/// Returns a map of normalised_name → summary string.
fn batch_pip_descriptions(names: &[&str]) -> HashMap<String, String> {
    if names.is_empty() {
        return HashMap::new();
    }

    let output = Command::new("pip")
        .arg("show")
        .args(names)
        .output();

    let Ok(output) = output else {
        return HashMap::new();
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut map = HashMap::new();
    let mut current_name: Option<String> = None;

    for line in text.lines() {
        if let Some(name) = line.strip_prefix("Name: ") {
            current_name = Some(normalize_name(name.trim()));
        } else if let Some(summary) = line.strip_prefix("Summary: ") {
            if let Some(ref name) = current_name {
                let summary = summary.trim();
                if !summary.is_empty() {
                    map.insert(name.clone(), summary.to_string());
                }
            }
        }
    }

    map
}

/// PEP 503 normalisation: lowercase and replace `[-_.]` sequences with `-`.
fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .replace(|c: char| c == '-' || c == '_' || c == '.', "-")
}
