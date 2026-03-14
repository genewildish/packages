//! Shared type definitions for pkgcheck.

use tabled::Tabled;

// ---------------------------------------------------------------------------
// Package-level status
// ---------------------------------------------------------------------------

/// Installation status of an individual package.
#[derive(Debug, Clone)]
pub enum PackageStatus {
    /// Package is installed at an acceptable version.
    Installed { version: String },
    /// Package is installed but at a different version than required.
    /// Not yet constructed — reserved for future semver-range checking.
    #[allow(dead_code)]
    OutOfDate { installed: String, required: String },
    /// Package is not installed at all.
    Missing,
}

// ---------------------------------------------------------------------------
// Aggregate project health
// ---------------------------------------------------------------------------

/// Overall health indicator for all detected dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverallStatus {
    /// Still scanning ecosystems (blinking green ●).
    Processing,
    /// Every package is installed and up to date (solid green ●).
    AllGood,
    /// Some packages are missing or outdated (orange ●).
    Partial,
    /// No required packages are installed (red ●).
    NoneInstalled,
}

// ---------------------------------------------------------------------------
// Per-package information
// ---------------------------------------------------------------------------

/// Full information about a single package dependency.
#[derive(Debug, Clone)]
pub struct PackageInfo {
    /// Package name as declared in the manifest (e.g. "serde", "express").
    pub name: String,
    /// Brief description of what the package does.
    pub description: String,
    /// Language / ecosystem label (e.g. "Node.js", "Python").
    pub language: String,
    /// Installed or required version string.
    pub version: String,
    /// Current installation status.
    pub status: PackageStatus,
    /// Percentage of the project's source files that import this package.
    pub usage_percent: f32,
    /// Names under which this package appears in import statements.
    /// Used by the scanner to map source-level imports back to packages.
    /// Example: Cargo crate "serde-json" → import alias "serde_json".
    pub import_aliases: Vec<String>,
}

// ---------------------------------------------------------------------------
// Table-rendering row
// ---------------------------------------------------------------------------

/// Row struct for rendering the summary table via `tabled`.
#[derive(Tabled)]
pub struct PackageRow {
    #[tabled(rename = "Package")]
    pub name: String,
    #[tabled(rename = "Description")]
    pub description: String,
    #[tabled(rename = "Language")]
    pub language: String,
    #[tabled(rename = "Version")]
    pub version: String,
    #[tabled(rename = "Status")]
    pub status_label: String,
    #[tabled(rename = "Usage %")]
    pub usage: String,
}

impl PackageInfo {
    /// Convert into a table-ready row for display.
    pub fn to_row(&self) -> PackageRow {
        let (version_display, status_label) = match &self.status {
            PackageStatus::Installed { version } => {
                (version.clone(), "✓ installed".to_string())
            }
            PackageStatus::OutOfDate {
                installed,
                required,
            } => (
                format!("{} (need {})", installed, required),
                "⚠ outdated".to_string(),
            ),
            PackageStatus::Missing => (self.version.clone(), "✗ missing".to_string()),
        };

        PackageRow {
            name: self.name.clone(),
            description: truncate_str(&self.description, 40),
            language: self.language.clone(),
            version: version_display,
            status_label,
            usage: format!("{:.1}%", self.usage_percent),
        }
    }
}

/// Truncate a string to `max_len` characters, appending "…" if longer.
/// Operates on Unicode char boundaries to avoid panics on multi-byte text.
fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{}…", truncated)
    }
}

// ---------------------------------------------------------------------------
// Ecosystem-level summary (used by the display layer)
// ---------------------------------------------------------------------------

/// Aggregated result of one ecosystem's package check.
#[derive(Debug, Clone)]
pub struct EcosystemSummary {
    /// Ecosystem name (e.g. "Node.js").
    pub name: String,
    /// Total number of declared packages.
    pub total: usize,
    /// Number currently installed (correct version).
    pub installed: usize,
    /// Number installed but at a different version.
    pub outdated: usize,
    /// Number not installed at all.
    pub missing: usize,
}

impl EcosystemSummary {
    /// Build a summary by inspecting a slice of package results.
    pub fn from_packages(name: &str, packages: &[PackageInfo]) -> Self {
        let mut installed = 0;
        let mut outdated = 0;
        let mut missing = 0;

        for pkg in packages {
            match &pkg.status {
                PackageStatus::Installed { .. } => installed += 1,
                PackageStatus::OutOfDate { .. } => outdated += 1,
                PackageStatus::Missing => missing += 1,
            }
        }

        Self {
            name: name.to_string(),
            total: packages.len(),
            installed,
            outdated,
            missing,
        }
    }
}
