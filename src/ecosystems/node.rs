//! Node.js ecosystem — detects `package.json` and checks `node_modules`.

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::Ecosystem;
use crate::types::{PackageInfo, PackageStatus};

pub struct NodeEcosystem;

impl Ecosystem for NodeEcosystem {
    fn name(&self) -> &str {
        "Node.js"
    }

    fn detect(&self, project_dir: &Path) -> bool {
        project_dir.join("package.json").exists()
    }

    fn check_packages(&self, project_dir: &Path) -> Result<Vec<PackageInfo>, String> {
        let manifest = project_dir.join("package.json");
        let content = fs::read_to_string(&manifest)
            .map_err(|e| format!("Failed to read {}: {}", manifest.display(), e))?;
        let parsed: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", manifest.display(), e))?;

        let mut packages = Vec::new();

        // Inspect both runtime and development dependencies.
        for section in &["dependencies", "devDependencies"] {
            if let Some(deps) = parsed.get(*section).and_then(Value::as_object) {
                for (name, ver_value) in deps {
                    let required = ver_value.as_str().unwrap_or("*").to_string();
                    let (status, version, description) =
                        read_installed_pkg(project_dir, name, &required);

                    packages.push(PackageInfo {
                        name: name.clone(),
                        description,
                        language: "Node.js".to_string(),
                        version,
                        status,
                        usage_percent: 0.0, // Filled by the scanner.
                        import_aliases: vec![name.clone()],
                    });
                }
            }
        }

        Ok(packages)
    }

    fn source_extensions(&self) -> &[&str] {
        &["js", "jsx", "ts", "tsx", "mjs", "cjs"]
    }

    fn import_patterns(&self) -> Vec<String> {
        vec![
            // ES module imports:  import X from 'pkg'  /  import X from 'pkg/sub'
            r#"import\s+.*?\s+from\s+['"]([^'"\./][^'"]*?)(?:/[^'"]*)?['"]"#.to_string(),
            // CommonJS:  require('pkg')
            r#"require\s*\(\s*['"]([^'"\./][^'"]*?)(?:/[^'"]*)?['"]\s*\)"#.to_string(),
            // Dynamic import:  import('pkg')
            r#"import\s*\(\s*['"]([^'"\./][^'"]*?)(?:/[^'"]*)?['"]\s*\)"#.to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read installed package data from `node_modules/<name>/package.json`.
/// Returns (status, version_string, description).
fn read_installed_pkg(
    project_dir: &Path,
    name: &str,
    required: &str,
) -> (PackageStatus, String, String) {
    let pkg_json = project_dir
        .join("node_modules")
        .join(name)
        .join("package.json");

    let Ok(content) = fs::read_to_string(&pkg_json) else {
        return (
            PackageStatus::Missing,
            required.to_string(),
            "N/A".to_string(),
        );
    };

    let Ok(parsed) = serde_json::from_str::<Value>(&content) else {
        return (
            PackageStatus::Missing,
            required.to_string(),
            "N/A".to_string(),
        );
    };

    let installed_ver = parsed
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let description = parsed
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("N/A")
        .to_string();

    // NOTE: A full semver-range comparison is out of scope for v0.1.
    // Any present version is treated as installed for now.
    let status = PackageStatus::Installed {
        version: installed_ver.clone(),
    };

    (status, installed_ver, description)
}
