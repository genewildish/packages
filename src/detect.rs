//! Project ecosystem detection.
//!
//! Scans the given directory for known manifest files and returns every
//! ecosystem whose manifests are present.

use std::path::Path;

use crate::ecosystems::{self, Ecosystem};

/// Return all ecosystems whose manifest files exist in `project_dir`.
pub fn detect_ecosystems(project_dir: &Path) -> Vec<Box<dyn Ecosystem>> {
    ecosystems::all_ecosystems()
        .into_iter()
        .filter(|eco| eco.detect(project_dir))
        .collect()
}
