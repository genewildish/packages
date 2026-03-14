//! Ecosystem detection and package checking.
//!
//! Each supported language ecosystem implements the [`Ecosystem`] trait, which
//! provides manifest detection, package status checking, and metadata for
//! the import-usage scanner.

pub mod go_lang;
pub mod node;
pub mod python;
pub mod ruby;
pub mod rust_lang;

use std::path::Path;

use crate::types::PackageInfo;

/// Trait that each language ecosystem must implement.
pub trait Ecosystem {
    /// Human-readable ecosystem name (e.g. "Node.js").
    fn name(&self) -> &str;

    /// Returns `true` when this ecosystem's manifest file(s) exist in
    /// `project_dir`.
    fn detect(&self, project_dir: &Path) -> bool;

    /// Inspect the manifest and local installation state, returning info for
    /// every declared dependency. Errors are returned as user-readable strings
    /// so callers can display them and continue with other ecosystems.
    fn check_packages(&self, project_dir: &Path) -> Result<Vec<PackageInfo>, String>;

    /// File extensions that belong to this ecosystem (without the leading dot).
    fn source_extensions(&self) -> &[&str];

    /// Regex patterns used to extract imported package names from source files.
    /// Each pattern **must** contain exactly one capture group at index 1 that
    /// yields the package-level name (not sub-module paths).
    fn import_patterns(&self) -> Vec<String>;
}

/// Returns one instance of every supported ecosystem.
pub fn all_ecosystems() -> Vec<Box<dyn Ecosystem>> {
    vec![
        Box::new(node::NodeEcosystem),
        Box::new(python::PythonEcosystem),
        Box::new(rust_lang::RustEcosystem),
        Box::new(go_lang::GoEcosystem),
        Box::new(ruby::RubyEcosystem),
    ]
}
