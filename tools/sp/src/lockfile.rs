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
            // '/'-joined so the hash doesn't depend on the host's separator.
            let rel = entry
                .strip_prefix(root)
                .unwrap_or(&entry)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let content =
                std::fs::read(&entry).map_err(|e| format!("failed to read {}: {}", entry.display(), e))?;
            hasher.update((rel.len() as u64).to_le_bytes());
            hasher.update(rel.as_bytes());
            hasher.update((content.len() as u64).to_le_bytes());
            hasher.update(&content);
        }
    }

    Ok(())
}

/// Generate a lockfile for a project and its dependencies.
pub fn generate(
    manifest: &crate::manifest::Manifest,
    resolved_deps: &[crate::resolver::ResolvedDep],
) -> Result<Lockfile, String> {
    let mut lockfile = Lockfile::new();

    // Root package: identity and direct dependencies, no content hash.
    let mut dep_names: Vec<String> = manifest.dependencies.keys().cloned().collect();
    dep_names.sort();
    lockfile.add_package(&manifest.package.name, &manifest.package.version, None, dep_names);

    // Only version dependencies are pinned. Path dependencies resolve from
    // the filesystem at build time and aren't stored
    // (docs/package-manager/DESIGN.md §2.4).
    for dep in resolved_deps {
        let Some(version) = &dep.resolved_version else {
            continue;
        };
        let dep_hash = compute_content_hash(&dep.root_path)?;

        // Get sub-deps from the dep's manifest
        let dep_manifest_path = dep.root_path.join("salt.toml");
        let sub_deps = if dep_manifest_path.exists() {
            if let Ok(sub_manifest) = crate::manifest::load(&dep_manifest_path) {
                let mut names: Vec<String> = sub_manifest.dependencies.keys().cloned().collect();
                names.sort();
                names
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        lockfile.add_package(&dep.name, version, Some(&dep_hash), sub_deps);
    }

    Ok(lockfile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_lockfile_roundtrip() {
        let tmp = crate::test_support::temp_path("sp_test_lockfile");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut lf = Lockfile::new();
        lf.add_package("test", "0.1.0", Some("sha256:abc123"), vec![]);
        lf.add_package("root", "1.0.0", None, vec![]);

        let lf_path = tmp.join("salt.lock");
        lf.save(&lf_path).unwrap();

        let loaded = Lockfile::load(&lf_path).unwrap();
        let pkg = loaded.packages.get("test").unwrap();
        assert_eq!(pkg.version, "0.1.0");
        assert_eq!(pkg.hash.as_deref(), Some("sha256:abc123"));
        assert_eq!(loaded.packages["root"].hash, None);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_content_hash_stable() {
        let tmp = crate::test_support::temp_path("sp_test_hash");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(tmp.join("src/main.salt"), "package main\nfn main() {}").unwrap();

        let hash1 = compute_content_hash(&tmp).unwrap();
        let hash2 = compute_content_hash(&tmp).unwrap();
        assert_eq!(hash1, hash2);
        assert!(hash1.starts_with("sha256:"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_content_hash_differs() {
        let tmp1 = crate::test_support::temp_path("sp_test_hash1");
        let tmp2 = crate::test_support::temp_path("sp_test_hash2");
        let _ = fs::remove_dir_all(&tmp1);
        let _ = fs::remove_dir_all(&tmp2);
        fs::create_dir_all(tmp1.join("src")).unwrap();
        fs::create_dir_all(tmp2.join("src")).unwrap();
        fs::write(tmp1.join("src/main.salt"), "package main\nfn main() { return 0; }").unwrap();
        fs::write(tmp2.join("src/main.salt"), "package main\nfn main() { return 1; }").unwrap();

        let hash1 = compute_content_hash(&tmp1).unwrap();
        let hash2 = compute_content_hash(&tmp2).unwrap();
        assert_ne!(hash1, hash2);

        let _ = fs::remove_dir_all(&tmp1);
        let _ = fs::remove_dir_all(&tmp2);
    }

    /// Writes `files` (path relative to the package root, content) under a
    /// fresh temp package directory.
    fn package_dir(tag: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let root = crate::test_support::temp_path(tag);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for (rel, content) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        root
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    // A reader holding the old file open keeps seeing all of it only on
    // platforms where renaming over an open file detaches it (unix).
    #[cfg(unix)]
    #[test]
    fn test_save_replaces_lockfile_atomically() {
        use std::io::Read;

        let tmp = crate::test_support::temp_path("sp_test_lockfile_atomic");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let path = tmp.join("salt.lock");

        let mut old = Lockfile::new();
        old.add_package("app", "1.0.0", None, vec![]);
        old.save(&path).unwrap();
        let old_text = fs::read_to_string(&path).unwrap();

        // Opened before the save, like a concurrent `sp build` reading it.
        let mut reader = fs::File::open(&path).unwrap();
        let mut new = Lockfile::new();
        new.add_package("app", "2.0.0", None, vec![]);
        new.save(&path).unwrap();

        let mut seen = String::new();
        reader.read_to_string(&mut seen).unwrap();
        assert_eq!(seen, old_text, "salt.lock must be replaced whole, not rewritten in place");
        assert!(fs::read_to_string(&path).unwrap().contains("version = \"2.0.0\""));
        assert_eq!(file_names(&tmp), ["salt.lock"], "no temporary file may be left behind");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_save_failure_removes_temporary_file() {
        let tmp = crate::test_support::temp_path("sp_test_lockfile_save_fails");
        let _ = fs::remove_dir_all(&tmp);
        // A non-empty directory where salt.lock goes: nothing can replace it.
        fs::create_dir_all(tmp.join("salt.lock/inside")).unwrap();

        let err = Lockfile::new().save(&tmp.join("salt.lock")).unwrap_err();
        assert!(err.contains("failed to write lockfile"), "{err}");
        assert_eq!(file_names(&tmp), ["salt.lock"], "the temporary file must be removed");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_save_orders_packages_deterministically() {
        let tmp = crate::test_support::temp_path("sp_test_lockfile_order");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Inserted in reverse. 8 names: an unordered map lands sorted by
        // chance with probability 1/8! (~2.5e-5).
        let names = ["theta", "eta", "zeta", "epsilon", "delta", "gamma", "beta", "alpha"];
        let mut lf = Lockfile::new();
        for n in names {
            lf.add_package(n, "1.0.0", Some("sha256:00"), vec![]);
        }
        let path = tmp.join("salt.lock");
        lf.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();

        let mut sorted = names.to_vec();
        sorted.sort();
        let positions: Vec<usize> = sorted
            .iter()
            .map(|n| text.find(&format!("[packages.{}]", n)).expect("package section present"))
            .collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "packages must serialize in sorted order so a committed salt.lock doesn't churn:\n{text}"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_generate_sorts_dependency_names() {
        let tmp = package_dir("sp_test_lockfile_dep_order", &[("src/main.salt", b"package main\nfn main() {}")]);
        fs::write(
            tmp.join("salt.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\
             theta = \"1\"\neta = \"1\"\nzeta = \"1\"\nepsilon = \"1\"\n\
             delta = \"1\"\ngamma = \"1\"\nbeta = \"1\"\nalpha = \"1\"\n",
        )
        .unwrap();
        let manifest = crate::manifest::load(&tmp.join("salt.toml")).unwrap();

        let lf = generate(&manifest, &[]).unwrap();
        let deps = &lf.packages["app"].deps;
        let mut sorted = deps.clone();
        sorted.sort();
        assert_eq!(deps, &sorted, "dependency names must be sorted for a deterministic salt.lock");
        assert_eq!(deps.len(), 8);

        let _ = fs::remove_dir_all(&tmp);
    }

    fn resolved(name: &str, root_path: PathBuf, version: Option<&str>) -> crate::resolver::ResolvedDep {
        crate::resolver::ResolvedDep {
            name: name.to_string(),
            source: format!("test:{}", name),
            root_path,
            resolved_version: version.map(str::to_string),
        }
    }

    #[test]
    fn test_generate_locks_only_version_deps() {
        let app = package_dir("sp_test_lock_scope_app", &[("src/main.salt", b"fn main() {}")]);
        fs::write(
            app.join("salt.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n\
             by_path = { path = \"../by_path\" }\nby_version = \"1.2\"\n",
        )
        .unwrap();
        let manifest = crate::manifest::load(&app.join("salt.toml")).unwrap();
        let path_dep = package_dir("sp_test_lock_scope_path", &[("src/lib.salt", b"P")]);
        let version_dep = package_dir("sp_test_lock_scope_version", &[("src/lib.salt", b"V")]);

        let lf = generate(
            &manifest,
            &[resolved("by_path", path_dep.clone(), None), resolved("by_version", version_dep.clone(), Some("1.2.0"))],
        )
        .unwrap();

        let names: Vec<&str> = lf.packages.keys().map(String::as_str).collect();
        assert_eq!(names, ["app", "by_version"], "path deps aren't locked (DESIGN.md §2.4)");
        assert_eq!(lf.packages["app"].hash, None, "root package isn't content-hashed");
        assert_eq!(lf.packages["app"].deps, ["by_path", "by_version"]);
        assert_eq!(lf.packages["by_version"].version, "1.2.0");
        assert!(lf.packages["by_version"].hash.as_deref().is_some_and(|h| h.starts_with("sha256:")));

        for dir in [app, path_dep, version_dep] {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn test_generate_sorts_transitive_dependency_names() {
        let app = package_dir("sp_test_lock_trans_app", &[("src/main.salt", b"fn main() {}")]);
        fs::write(app.join("salt.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n").unwrap();
        let manifest = crate::manifest::load(&app.join("salt.toml")).unwrap();
        let dep = package_dir("sp_test_lock_trans_dep", &[("src/lib.salt", b"D")]);
        fs::write(
            dep.join("salt.toml"),
            "[package]\nname = \"dep\"\nversion = \"2.0.0\"\n\n[dependencies]\n\
             theta = \"1\"\neta = \"1\"\nzeta = \"1\"\nepsilon = \"1\"\n\
             delta = \"1\"\ngamma = \"1\"\nbeta = \"1\"\nalpha = \"1\"\n",
        )
        .unwrap();

        let lf = generate(&manifest, &[resolved("dep", dep.clone(), Some("2.0.0"))]).unwrap();

        let deps = &lf.packages["dep"].deps;
        let mut sorted = deps.clone();
        sorted.sort();
        assert_eq!(deps, &sorted, "a dependency's own deps must be sorted too");
        assert_eq!(deps.len(), 8);

        let _ = fs::remove_dir_all(app);
        let _ = fs::remove_dir_all(dep);
    }

    #[test]
    fn test_content_hash_distinguishes_file_boundaries() {
        // Without length-delimiting, one file whose content spells out the
        // next file's name streams the same bytes as two files:
        // "a.salt" + "Xb.saltY" == "a.salt" + "X" + "b.salt" + "Y".
        let one = package_dir("sp_test_hash_boundary_one", &[("src/a.salt", b"Xb.saltY")]);
        let two = package_dir("sp_test_hash_boundary_two", &[("src/a.salt", b"X"), ("src/b.salt", b"Y")]);

        assert_ne!(compute_content_hash(&one).unwrap(), compute_content_hash(&two).unwrap());

        let _ = fs::remove_dir_all(&one);
        let _ = fs::remove_dir_all(&two);
    }

    #[test]
    fn test_content_hash_distinguishes_directories() {
        // Same file name and content in different directories is a
        // different source tree; hashing bare file names can't tell.
        let x = package_dir("sp_test_hash_dir_x", &[("src/x/m.salt", b"package x.m")]);
        let y = package_dir("sp_test_hash_dir_y", &[("src/y/m.salt", b"package x.m")]);

        assert_ne!(compute_content_hash(&x).unwrap(), compute_content_hash(&y).unwrap());

        let _ = fs::remove_dir_all(&x);
        let _ = fs::remove_dir_all(&y);
    }

    #[test]
    fn test_content_hash_covers_packages_without_src() {
        // A package with no src/ (entry at the root) is a valid layout; two
        // different ones must not share a hash.
        let one = package_dir("sp_test_hash_nosrc_one", &[("main.salt", b"fn main() -> i32 { return 1; }")]);
        let two = package_dir("sp_test_hash_nosrc_two", &[("main.salt", b"fn main() -> i32 { return 2; }")]);

        assert_ne!(compute_content_hash(&one).unwrap(), compute_content_hash(&two).unwrap());

        let _ = fs::remove_dir_all(&one);
        let _ = fs::remove_dir_all(&two);
    }

    #[test]
    fn test_content_hash_covers_root_files_beside_src() {
        // `sp publish` packs root-level .salt files too, so they're part of
        // the package's identity even when src/ exists.
        let a = package_dir("sp_test_hash_root_a", &[("src/lib.salt", b"L"), ("main.salt", b"A")]);
        let b = package_dir("sp_test_hash_root_b", &[("src/lib.salt", b"L"), ("main.salt", b"B")]);

        assert_ne!(compute_content_hash(&a).unwrap(), compute_content_hash(&b).unwrap());

        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn test_content_hash_skips_what_publish_skips() {
        // Same file set as `sp publish`: tests/, target/ and dot-dirs aren't
        // part of the package.
        let base = package_dir("sp_test_hash_skip_base", &[("src/lib.salt", b"L")]);
        let extra = package_dir(
            "sp_test_hash_skip_extra",
            &[
                ("src/lib.salt", b"L"),
                ("tests/t.salt", b"T"),
                ("src/tests/u.salt", b"U"),
                ("target/gen.salt", b"G"),
                (".cache/c.salt", b"C"),
            ],
        );

        assert_eq!(compute_content_hash(&base).unwrap(), compute_content_hash(&extra).unwrap());

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&extra);
    }

    #[test]
    fn test_content_hash_frames_content_length() {
        // Content that embeds the next record's path header: without a
        // content length prefix this streams the same bytes as two files.
        let next = b"src/b.salt";
        let mut content = b"X".to_vec();
        content.extend_from_slice(&(next.len() as u64).to_le_bytes());
        content.extend_from_slice(next);
        content.extend_from_slice(b"Y");
        let one = package_dir("sp_test_hash_frame_one", &[("src/a.salt", &content)]);
        let two = package_dir("sp_test_hash_frame_two", &[("src/a.salt", b"X"), ("src/b.salt", b"Y")]);

        assert_ne!(compute_content_hash(&one).unwrap(), compute_content_hash(&two).unwrap());

        let _ = fs::remove_dir_all(&one);
        let _ = fs::remove_dir_all(&two);
    }
}
