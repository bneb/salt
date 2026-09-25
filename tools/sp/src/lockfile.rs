//! Lockfile generation for reproducible builds.
//!
//! The salt.lock file records exact versions and content hashes for
//! all packages in the dependency tree.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A lockfile entry for a single package.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LockedPackage {
    pub version: String,
    /// Content hash in format "sha256:<hex>". Absent for the root package:
    /// its source is the tree being built, so hashing it would rewrite the
    /// committed lockfile on every edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
}

/// The complete lockfile.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Lockfile {
    // BTreeMap, not HashMap: serialization order must be deterministic or a
    // committed salt.lock rewrites itself differently on every build.
    pub packages: BTreeMap<String, LockedPackage>,
}

impl Lockfile {
    /// Create an empty lockfile.
    pub fn new() -> Self {
        Self {
            packages: BTreeMap::new(),
        }
    }

    /// Load a lockfile from disk.
    #[allow(dead_code)]
    pub fn load(path: &Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("failed to read lockfile: {}", e))?;
        toml::from_str(&content).map_err(|e| format!("failed to parse lockfile: {}", e))
    }

    /// Save the lockfile to disk, atomically: the content goes to a
    /// temporary file that is synced, then renamed over `path`, so a reader
    /// or an interrupted build sees the old file or the new one, never a
    /// partial one.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create lockfile dir: {}", e))?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize lockfile: {}", e))?;

        // Beside `path`, so the rename stays on one filesystem; the pid keeps
        // concurrent builds out of each other's temporary file.
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(format!(".{}.tmp", std::process::id()));
        let tmp = PathBuf::from(tmp);
        std::fs::File::create(&tmp)
            .and_then(|mut file| {
                file.write_all(content.as_bytes())?;
                file.sync_all()
            })
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("failed to write lockfile: {}", e)
            })
    }

    /// Insert or update a package entry.
    pub fn add_package(&mut self, name: &str, version: &str, hash: Option<&str>, deps: Vec<String>) {
        self.packages.insert(
            name.to_string(),
            LockedPackage {
                version: version.to_string(),
                hash: hash.map(str::to_string),
                deps,
            },
        );
    }
}

/// Compute the content hash (SHA-256) of a package's source.
///
/// Covers the .salt files `sp publish` packs: everything under the package
/// root except dot-directories, `target/` and `tests/`, visited in sorted
/// order. Each file contributes its length-prefixed root-relative path and
/// length-prefixed content, so distinct source trees (with UTF-8 names)
/// can't stream the same bytes. Not normalized: non-UTF-8 names convert
/// lossily, and Unicode normalization (NFC vs NFD) or CRLF line endings
/// change the hash. Returns "sha256:<hex>".
pub fn compute_content_hash(package_dir: &Path) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hash_source_files(&mut hasher, package_dir, package_dir)?;
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Recursively hash all .salt files under `dir`, sorted for determinism.
fn hash_source_files(hasher: &mut Sha256, root: &Path, dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read {}: {}", dir.display(), e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();

    entries.sort();

    for entry in entries {
        if entry.is_dir() {
            let name = entry.file_name().unwrap().to_string_lossy();
            if !name.starts_with('.') && name != "target" && name != "tests" {
                hash_source_files(hasher, root, &entry)?;
            }
        } else if entry.extension().is_some_and(|e| e == "salt") {
            hash_record(hasher, root, &entry)?;
        }
    }

    Ok(())
}

/// Feed one file as its length-prefixed root-relative path followed by its
/// length-prefixed content. The path is '/'-joined so the hash doesn't
/// depend on the host's separator.
fn hash_record(hasher: &mut Sha256, root: &Path, file: &Path) -> Result<(), String> {
    let rel = file
        .strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let content =
        std::fs::read(file).map_err(|e| format!("failed to read {}: {}", file.display(), e))?;
    hasher.update((rel.len() as u64).to_le_bytes());
    hasher.update(rel.as_bytes());
    hasher.update((content.len() as u64).to_le_bytes());
    hasher.update(&content);
    Ok(())
}

/// Generate a lockfile for a project and its dependencies.
pub fn generate(
    manifest: &crate::manifest::Manifest,
    resolved_deps: &[crate::resolver::ResolvedDep],
) -> Result<Lockfile, String> {
    let mut lockfile = Lockfile::new();

    // Root package: identity and direct dependencies, no content hash.
    let dep_names = manifest.dependencies.keys().cloned().collect();
    lockfile.add_package(&manifest.package.name, &manifest.package.version, None, dep_names);

    // Only version dependencies are pinned. Path dependencies resolve from
    // the filesystem at build time and aren't stored
    // (docs/package-manager/DESIGN.md §2.4).
    for dep in resolved_deps {
        let Some(version) = &dep.resolved_version else {
            continue;
        };
        let dep_hash = compute_content_hash(&dep.root_path)?;
        let sub_deps = declared_dependencies(&dep.root_path);
        lockfile.add_package(&dep.name, version, Some(&dep_hash), sub_deps);
    }

    Ok(lockfile)
}

/// Dependency names declared by the salt.toml in `dir` (sorted: the
/// manifest's map is a BTreeMap), or none if it can't be loaded.
fn declared_dependencies(dir: &Path) -> Vec<String> {
    crate::manifest::load(&dir.join("salt.toml"))
        .map(|m| m.dependencies.keys().cloned().collect())
        .unwrap_or_default()
}

/// Generate salt.lock and save it in `project_dir`: the root package plus
/// the resolved version and content hash of each version dependency.
/// Nothing reads it back yet.
pub fn write(
    manifest: &crate::manifest::Manifest,
    project_dir: &Path,
    resolved: &[crate::resolver::ResolvedDep],
) -> Result<(), String> {
    generate(manifest, resolved)?.save(&project_dir.join("salt.lock"))
}

/// `write`, reporting the outcome. A failure only warns: the build doesn't
/// depend on salt.lock.
pub fn write_or_warn(
    manifest: &crate::manifest::Manifest,
    project_dir: &Path,
    resolved: &[crate::resolver::ResolvedDep],
) {
    match write(manifest, project_dir, resolved) {
        Ok(()) => println!("   🔒 Wrote salt.lock"),
        Err(e) => eprintln!("\x1b[1;33mwarning\x1b[0m: salt.lock not written: {}", e),
    }
}
