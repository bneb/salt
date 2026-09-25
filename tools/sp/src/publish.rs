//! Package publishing — creates .tar.gz archives for distribution.
//!
//! Published packages are stored locally in ~/.salt/publish/.
//! The resolver extracts them to ~/.salt/packages/<name>-<version>/
//! for use as dependencies.

use crate::manifest::{Dependency, Manifest};
use crate::semver;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Get the publish directory path (~/.salt/publish/).
fn publish_dir() -> Result<PathBuf, String> {
    let home =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    let dir = PathBuf::from(home).join(".salt").join("publish");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create publish dir: {}", e))?;
    Ok(dir)
}

/// Get the extracted packages cache directory (~/.salt/packages/).
fn packages_dir() -> Result<PathBuf, String> {
    let home =
        std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    Ok(PathBuf::from(home).join(".salt").join("packages"))
}

/// Publish the current project as a .tar.gz archive.
///
/// Output: ~/.salt/publish/<name>-<version>.tar.gz
/// Archive contains: salt.toml, all .salt files from src/
pub fn publish(manifest: &Manifest, project_dir: &Path) -> Result<(), String> {
    // Before creating the archive, which would replace one already published.
    reject_path_dependencies(manifest)?;

    let pub_dir = publish_dir()?;
    let archive_name = format!("{}-{}.tar.gz", manifest.package.name, manifest.package.version);
    let archive_path = pub_dir.join(&archive_name);

    // Collect source files
    let mut files = Vec::new();
    collect_source_files(project_dir, &mut files)?;

    // Include salt.toml
    let manifest_path = project_dir.join("salt.toml");
    if manifest_path.exists() {
        files.push(manifest_path);
    }

    // Create tar.gz
    let file =
        std::fs::File::create(&archive_path).map_err(|e| format!("failed to create archive: {}", e))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);

    // Build file manifest for salt-publish.toml
    let mut manifest_lines = String::new();
    manifest_lines.push_str(&format!(
        "[package]\nname = \"{}\"\nversion = \"{}\"\n\n[files]\n",
        manifest.package.name, manifest.package.version
    ));

    for file_path in &files {
        let relative = file_path
            .strip_prefix(project_dir)
            .map_err(|_| "failed to compute relative path".to_string())?;
        let relative_str = relative.to_string_lossy().to_string();

        // Hash file content
        let content =
            std::fs::read(file_path).map_err(|e| format!("failed to read {}: {}", file_path.display(), e))?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash = hex::encode(hasher.finalize());

        manifest_lines.push_str(&format!("{} = \"sha256:{}\"\n", relative_str, hash));

        // Add to tar archive
        tar.append_path_with_name(file_path, &relative_str)
            .map_err(|e| format!("failed to add {} to archive: {}", file_path.display(), e))?;
    }

    // Add the manifest file to the archive
    let manifest_bytes = manifest_lines.into_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append_data(&mut header, "salt-publish.toml", manifest_bytes.as_slice())
        .map_err(|e| format!("failed to write manifest: {}", e))?;

    tar.finish().map_err(|e| format!("failed to finalize archive: {}", e))?;

    println!(
        "📦 Published \x1b[1m{}\x1b[0m v{} to {}",
        manifest.package.name,
        manifest.package.version,
        archive_path.display()
    );

    Ok(())
}

/// Consumers resolve a published package's dependencies from its copy
/// extracted to ~/.salt/packages/, where a relative path leads nowhere, so
/// a path dependency would break every one of them. Dev-dependencies are
/// fine: nothing resolves a dependency's dev-dependencies.
fn reject_path_dependencies(manifest: &Manifest) -> Result<(), String> {
    let paths: Vec<String> = manifest
        .dependencies
        .iter()
        .filter_map(|(name, dep)| match dep {
            Dependency::Path { path, .. } => Some(format!("\n    {} = {{ path = \"{}\" }}", name, path)),
            _ => None,
        })
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    Err(format!(
        "cannot publish '{}' with path dependencies:{}\n  \
         Packages that use '{}' resolve its dependencies from the copy extracted to \
         ~/.salt/packages/, where these paths don't exist.\n  \
         Hint: publish each of them with `sp publish` and depend on it by version.",
        manifest.package.name,
        paths.concat(),
        manifest.package.name
    ))
}

/// Collect all .salt files under a directory.
fn collect_source_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("failed to read {}: {}", dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read error: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.starts_with('.') && name != "target" && name != "tests" {
                collect_source_files(&path, files)?;
            }
        } else if path.extension().is_some_and(|e| e == "salt") {
            files.push(path);
        }
    }
    Ok(())
}

/// Find all published versions of a package.
///
/// Returns (version, archive_path) for every published version of the
/// given package name, sorted by version, then by path: archives published
/// as "0.1" and "0.1.0" both hold 0.1.0, and must come out in the same
/// order on every run.
pub fn find_published(name: &str) -> Result<Vec<(semver::Version, PathBuf)>, String> {
    let dir = publish_dir()?;
    let mut results = Vec::new();

    if !dir.exists() {
        return Ok(results);
    }

    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("failed to read publish dir: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read error: {}", e))?;
        let path = entry.path();

        if path.extension().is_some_and(|e| e == "gz") {
            let filename = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_suffix(".tar"))
                .unwrap_or("");

            // Parse "<name>-<version>" from filename
            let prefix = format!("{}-", name);
            if let Some(ver_str) = filename.strip_prefix(&prefix) {
                if let Ok(ver) = semver::parse_version(ver_str) {
                    results.push((ver, path));
                }
            }
        }
    }

    results.sort();
    Ok(results)
}

/// Extract a published package to the local packages cache.
///
/// `archive_path` is the archive `find_published` listed for `version`. It
/// can't be rebuilt from `version`: the file name keeps the version as it
/// was published ("two-0.1.tar.gz"), while `version` is normalized (0.1.0).
///
/// Returns the path to the extracted package directory.
pub fn extract_package(
    name: &str,
    version: &semver::Version,
    archive_path: &Path,
) -> Result<PathBuf, String> {
    let pkg_dir = packages_dir()?.join(format!("{}-{}", name, version));

    // Skip if already extracted
    if pkg_dir.join("salt.toml").exists() {
        return Ok(pkg_dir);
    }

    // Extract
    let file = std::fs::File::open(archive_path)
        .map_err(|e| format!("failed to open {}: {}", archive_path.display(), e))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    std::fs::create_dir_all(&pkg_dir)
        .map_err(|e| format!("failed to create package dir: {}", e))?;

    archive
        .unpack(&pkg_dir)
        .map_err(|e| format!("failed to extract {}: {}", archive_path.display(), e))?;

    Ok(pkg_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_collect_source_files_finds_salt_files() {
        let tmp = crate::test_support::temp_path("sp_test_publish_collect");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(tmp.join("src/main.salt"), "package main").unwrap();
        fs::write(tmp.join("src/lib.salt"), "package lib").unwrap();
        fs::write(tmp.join("README.md"), "# readme").unwrap();

        let mut files = Vec::new();
        collect_source_files(&tmp, &mut files).unwrap();

        assert!(files.iter().any(|f| f.ends_with("src/main.salt")));
        assert!(files.iter().any(|f| f.ends_with("src/lib.salt")));
        // README should not be included
        assert!(!files.iter().any(|f| f.ends_with("README.md")));

        let _ = fs::remove_dir_all(&tmp);
    }

    /// A package named `withpath` under `root`, with `manifest_tail`
    /// appended to its salt.toml.
    fn package_with(root: &Path, manifest_tail: &str) -> (PathBuf, Manifest) {
        let dir = root.join("withpath");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.salt"), "package withpath\n").unwrap();
        fs::write(
            dir.join("salt.toml"),
            format!("[package]\nname = \"withpath\"\nversion = \"1.0.0\"\n\n{}", manifest_tail),
        )
        .unwrap();
        let manifest = crate::manifest::load(&dir.join("salt.toml")).unwrap();
        (dir, manifest)
    }

    #[test]
    fn test_publish_rejects_path_dependencies() {
        let tmp = crate::test_support::temp_path("sp_test_publish_path_dep");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        let (dir, manifest) = package_with(
            &tmp,
            "[dependencies]\nhelper = { path = \"../helper\" }\nother = \"1\"\n",
        );

        let err = publish(&manifest, &dir).expect_err("no consumer could resolve ../helper");
        assert!(err.contains("cannot publish 'withpath'"), "{err}");
        assert!(err.contains("helper = { path = \"../helper\" }"), "{err}");
        assert!(!err.contains("other"), "only path dependencies are the problem: {err}");
        assert!(
            !tmp.join("home/.salt/publish/withpath-1.0.0.tar.gz").exists(),
            "nothing may be published"
        );

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_publish_allows_path_dev_dependencies() {
        let tmp = crate::test_support::temp_path("sp_test_publish_path_dev_dep");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        // Consumers never resolve a dependency's dev-dependencies.
        let (dir, manifest) = package_with(&tmp, "[dev-dependencies]\nhelper = { path = \"../helper\" }\n");

        publish(&manifest, &dir).expect("a path dev-dependency doesn't reach consumers");
        assert!(tmp.join("home/.salt/publish/withpath-1.0.0.tar.gz").exists());

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_published_empty_dir() {
        let tmp = crate::test_support::temp_path("sp_test_find_published_empty");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp);

        // $HOME/.salt/publish doesn't exist yet.
        let results = find_published("nonexistent").expect("a missing publish dir is not an error");
        assert!(results.is_empty());

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_published_sorts_by_version_then_path() {
        let tmp = crate::test_support::temp_path("sp_test_find_published_order");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp);
        let dir = tmp.join(".salt/publish");
        fs::create_dir_all(&dir).unwrap();
        // Only the names matter. two-0.1 and two-0.1.0 both hold 0.1.0.
        for archive in ["two-0.2.tar.gz", "two-0.1.tar.gz", "two-0.1.0.tar.gz", "two-0.0.9.tar.gz"] {
            fs::write(dir.join(archive), b"").unwrap();
        }

        let found: Vec<(String, String)> = find_published("two")
            .unwrap()
            .into_iter()
            .map(|(v, p)| (v.to_string(), p.file_name().unwrap().to_string_lossy().into_owned()))
            .collect();
        let expected = [
            ("0.0.9", "two-0.0.9.tar.gz"),
            ("0.1.0", "two-0.1.0.tar.gz"),
            ("0.1.0", "two-0.1.tar.gz"),
            ("0.2.0", "two-0.2.tar.gz"),
        ]
        .map(|(v, f)| (v.to_string(), f.to_string()));
        assert_eq!(found, expected);

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }
}
