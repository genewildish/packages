//! Source-file scanner for import-usage analysis.
//!
//! Walks the project tree, matches each ecosystem's import patterns against
//! source files, and computes what percentage of files use each package.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::types::PackageInfo;

/// Directories that should never be descended into during source walks.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".tox",
    ".mypy_cache",
    ".eggs",
    ".bundle",
];

/// Scan source files in `project_dir` and update each package's
/// `usage_percent` field in-place.
///
/// For every file whose extension matches `extensions`, apply each of the
/// `import_patterns` regexes to extract imported names, then cross-reference
/// those names against the `import_aliases` declared on each package.
pub fn compute_usage(
    project_dir: &Path,
    packages: &mut [PackageInfo],
    extensions: &[&str],
    import_patterns: &[String],
) {
    // Compile regex patterns (silently skip any that fail to compile).
    let regexes: Vec<Regex> = import_patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();

    if regexes.is_empty() || packages.is_empty() {
        return;
    }

    // Collect every source file that matches the requested extensions.
    let source_files = walk_source_files(project_dir, extensions);
    let total_files = source_files.len();
    if total_files == 0 {
        return;
    }

    // Map from lowercased import alias → package index for fast lookup.
    let mut alias_map: HashMap<String, usize> = HashMap::new();
    for (i, pkg) in packages.iter().enumerate() {
        for alias in &pkg.import_aliases {
            alias_map.insert(alias.to_lowercase(), i);
        }
    }

    // Per-package file counters.
    let mut counts: Vec<usize> = vec![0; packages.len()];

    for file_path in &source_files {
        let Ok(content) = fs::read_to_string(file_path) else {
            continue;
        };

        // Track which packages have already been counted for this file
        // so that multiple imports of the same package don't inflate the count.
        let mut seen_in_file = vec![false; packages.len()];

        for re in &regexes {
            for cap in re.captures_iter(&content) {
                if let Some(m) = cap.get(1) {
                    let imported = m.as_str().to_lowercase();

                    // Exact match against known aliases.
                    if let Some(&idx) = alias_map.get(&imported) {
                        seen_in_file[idx] = true;
                    }

                    // For Go-style full paths, also check if any alias is a
                    // prefix of the imported path (handles sub-package imports).
                    for (alias, &idx) in &alias_map {
                        if imported.starts_with(alias.as_str()) && !seen_in_file[idx] {
                            // Make sure it's a true prefix (at a '/' boundary).
                            if imported.len() == alias.len()
                                || imported.as_bytes().get(alias.len()) == Some(&b'/')
                            {
                                seen_in_file[idx] = true;
                            }
                        }
                    }
                }
            }
        }

        for (idx, &seen) in seen_in_file.iter().enumerate() {
            if seen {
                counts[idx] += 1;
            }
        }
    }

    // Convert raw counts into percentages.
    for (i, pkg) in packages.iter_mut().enumerate() {
        pkg.usage_percent = (counts[i] as f32 / total_files as f32) * 100.0;
    }
}

// ---------------------------------------------------------------------------
// File-tree walker
// ---------------------------------------------------------------------------

/// Recursively collect files whose extension matches `extensions`, skipping
/// directories listed in [`SKIP_DIRS`].
fn walk_source_files(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut results = Vec::new();
    walk_recursive(dir, extensions, &mut results);
    results
}

fn walk_recursive(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // Skip well-known non-source directories.
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if SKIP_DIRS.contains(&name) {
                    continue;
                }
            }
            walk_recursive(&path, extensions, out);
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext) {
                    out.push(path);
                }
            }
        }
    }
}
