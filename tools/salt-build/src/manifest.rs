//! Salt Manifest Parser — reads salt.toml
//!
//! Defines the structure of a Salt project manifest and provides
//! loading/parsing functionality.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Top-level salt.toml manifest structure.
#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
}

/// Package metadata.
#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    /// Entry point (relative to project dir)
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_entry() -> String {
    "src/main.salt".to_string()
}

/// A dependency specification.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
    /// Local path dependency
    Path { path: String },
    /// Version string (future: registry support)
    Version(String),
}

impl Dependency {
    /// Get the local path for a path dependency.
    pub fn local_path(&self) -> Option<&str> {
        match self {
            Dependency::Path { path } => Some(path),
            _ => None,
        }
    }
}

/// Load and parse a salt.toml manifest file.
pub fn load(path: &Path) -> Result<Manifest, String> {
    if !path.exists() {
        return Err(format!(
            "No salt.toml found at {}. Run `salt init <name>` to create a project.",
            path.display()
        ));
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    let manifest: Manifest = toml::from_str(&content)
        .map_err(|e| format!("Failed to parse salt.toml: {}", e))?;

    // Validate
    if manifest.package.name.is_empty() {
        return Err("package.name cannot be empty".to_string());
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_manifest() {
        let toml = r#"
[package]
name = "my_app"
version = "0.1.0"
"#;
        let manifest: Manifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.package.name, "my_app");
        assert_eq!(manifest.package.version, "0.1.0");
        assert_eq!(manifest.package.entry, "src/main.salt");
    }

    #[test]
    fn test_parse_with_dependencies() {
        let toml = r#"
[package]
name = "my_app"
version = "0.1.0"
entry = "src/main.salt"

[dependencies]
mathlib = { path = "../mathlib" }
"#;
        let manifest: Manifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.dependencies.len(), 1);
        let dep = manifest.dependencies.get("mathlib").unwrap();
        assert_eq!(dep.local_path(), Some("../mathlib"));
    }

    #[test]
    fn test_parse_custom_entry() {
        let toml = r#"
[package]
name = "lib"
version = "1.0.0"
entry = "lib.salt"
"#;
        let manifest: Manifest = toml::from_str(toml).unwrap();
        assert_eq!(manifest.package.entry, "lib.salt");
    }
}
