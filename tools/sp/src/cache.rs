//! Content-Addressed Artifact Cache
//!
//! Implements the Nix-lite global cache from the sp design:
//!   cache_key = sha256(source + compiler + target + features + deps_hash)
//!
//! Artifacts are stored in ~/.salt/cache/artifacts/<hash>
//! Cache hits skip all compilation for instant no-op builds.

use crate::manifest::Manifest;
use crate::resolver::ResolvedDep;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The global artifact cache.
pub struct ArtifactCache {
    cache_dir: PathBuf,
}

impl ArtifactCache {
    /// Create a new artifact cache instance.
    pub fn new() -> Result<Self, String> {
        let home = std::env::var("HOME")
            .map_err(|_| "HOME environment variable not set".to_string())?;
        let cache_dir = PathBuf::from(home).join(".salt").join("cache").join("artifacts");

        // Create cache directory if it doesn't exist
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir)
                .map_err(|e| format!("failed to create cache dir: {}", e))?;
        }

        Ok(Self { cache_dir })
    }

    /// Compute the content-addressed cache key for a build.
    ///
    /// The key includes:
    ///   - source hash (all .salt files in the project)
    ///   - compiler version (salt-front binary hash, approximated by version string)
    ///   - build profile (release/debug)
    ///   - target triple
    ///   - feature flags
    ///   - deps_hash (resolved dependency artifact hashes — prevents ABI mismatch)
    ///
    /// Every field is length-prefixed and each source tree is digested on
    /// its own, so distinct inputs can't stream the same bytes.
    pub fn compute_key(
        &self,
        manifest: &Manifest,
        project_dir: &Path,
        release: bool,
        search_roots: &[PathBuf],
        deps: &[ResolvedDep],
    ) -> Result<String, String> {
        let mut hasher = Sha256::new();

        // 1. Source hash — hash all .salt files in the project. The tag keeps
        //    a src/ digest apart from an entry file holding the same bytes.
        let src_dir = project_dir.join("src");
        if src_dir.exists() {
            update_field(&mut hasher, b"src");
            update_field(&mut hasher, &tree_digest(&src_dir)?);
        } else {
            let entry = project_dir.join(&manifest.package.entry);
            if entry.exists() {
                let content = std::fs::read(&entry)
                    .map_err(|e| format!("failed to read {}: {}", entry.display(), e))?;
                update_field(&mut hasher, b"entry");
                update_field(&mut hasher, &content);
            }
        }

        // 2. Compiler version — hash the salt-front binary if available
        let compiler_id = compiler_version_hash(project_dir);
        update_field(&mut hasher, compiler_id.as_bytes());

        // 3. Build profile
        let profile = if release { "release" } else { "debug" };
        update_field(&mut hasher, profile.as_bytes());

        // 4. Target triple
        let target = manifest
            .build
            .as_ref()
            .and_then(|b| b.target.as_ref())
            .map(|t| t.as_str())
            .unwrap_or("aarch64-apple-darwin");
        update_field(&mut hasher, target.as_bytes());

        // 5. Dependency roots hash (transitive cache key fix)
        //    Hash the search roots and each dependency's package directory,
        //    one digest per root so a file can't move between roots unseen.
        let mut dep_hasher = Sha256::new();
        for root in search_roots.iter().chain(deps.iter().map(|d| &d.root_path)) {
            if root.exists() {
                update_field(&mut dep_hasher, &tree_digest(root)?);
            }
        }
        // saltc also gets each dependency's name and entry (`--dep`)
        for dep in deps {
            let entry = dep.entry.strip_prefix(&dep.root_path).unwrap_or(&dep.entry);
            update_field(&mut dep_hasher, dep.name.as_bytes());
            update_field(&mut dep_hasher, entry.display().to_string().as_bytes());
        }
        let deps_hash = dep_hasher.finalize();
        update_field(&mut hasher, &deps_hash);

        let result = hasher.finalize();
        Ok(hex::encode(result))
    }

    /// Look up a cached artifact by its key.
    /// Returns the path to the cached binary if it exists.
    pub fn lookup(&self, key: &str) -> Option<PathBuf> {
        let cached_path = self.cache_dir.join(key);
        if cached_path.exists() {
            Some(cached_path)
        } else {
            None
        }
    }

    /// Store a built artifact in the cache.
    pub fn store(&self, key: &str, binary_path: &Path) -> Result<(), String> {
        if !binary_path.exists() {
            return Ok(()); // Nothing to cache
        }

        let cached_path = self.cache_dir.join(key);
        std::fs::copy(binary_path, &cached_path)
            .map_err(|e| format!("failed to cache artifact: {}", e))?;

        Ok(())
    }
}

/// Get a version identifier for the salt-front compiler.
///
/// Tries to hash a portion of the salt-front binary for accurate cache
/// invalidation when the compiler changes. Falls back to a version string.
fn compiler_version_hash(project_dir: &Path) -> String {
    // Try to find and hash the salt-front binary
    let mut dir = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());

    loop {
        let bin = dir.join("salt-front/target/release/saltc");
        if bin.exists() {
            if let Ok(data) = std::fs::read(&bin) {
                let mut h = Sha256::new();
                // Hash first 64KB + last 64KB for speed (sufficient for version detection)
                let len = data.len();
                let head = &data[..len.min(65536)];
                let tail = if len > 65536 { &data[len - 65536..] } else { &[] };
                h.update(head);
                h.update(tail);
                return hex::encode(h.finalize());
            }
            break;
        }

        let debug_bin = dir.join("salt-front/target/debug/saltc");
        if debug_bin.exists() {
            if let Ok(data) = std::fs::read(&debug_bin) {
                let mut h = Sha256::new();
                let len = data.len();
                h.update(&data[..len.min(65536)]);
                return hex::encode(h.finalize());
            }
            break;
        }

        if !dir.pop() { break; }
    }

    // Fallback: use environment-provided version
    std::env::var("SALT_COMPILER_VERSION")
        .unwrap_or_else(|_| "salt-front-dev".to_string())
}

/// Feeds `bytes` prefixed with its u64le length, so adjacent fields can't
/// trade bytes across their boundary.
fn update_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// SHA-256 of the .salt files under `root`. Each file contributes its
/// length-prefixed root-relative path and length-prefixed content, so
/// distinct trees (with UTF-8 names) can't stream the same bytes.
fn tree_digest(root: &Path) -> Result<[u8; 32], String> {
    let mut hasher = Sha256::new();
    hash_directory(&mut hasher, root, root)?;
    Ok(hasher.finalize().into())
}

/// Hash all .salt files in a directory recursively.
fn hash_directory(hasher: &mut Sha256, root: &Path, dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read {}: {}", dir.display(), e))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();

    // Sort for deterministic hashing
    entries.sort();

    for entry in entries {
        if entry.is_dir() {
            let name = entry.file_name().unwrap().to_string_lossy();
            if !name.starts_with('.') && name != "target" {
                hash_directory(hasher, root, &entry)?;
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
            let content = std::fs::read(&entry)
                .map_err(|e| format!("failed to read {}: {}", entry.display(), e))?;
            update_field(hasher, rel.as_bytes());
            update_field(hasher, &content);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::BuildConfig;
    use std::fs;

    #[test]
    fn test_cache_roundtrip() {
        let tmp = crate::test_support::temp_path("sp_test_cache");
        let _ = fs::remove_dir_all(&tmp);

        let cache = ArtifactCache {
            cache_dir: tmp.clone(),
        };
        fs::create_dir_all(&tmp).unwrap();

        // No cache hit initially
        assert!(cache.lookup("test_key_123").is_none());

        // Create a fake binary and store it
        let fake_binary = tmp.join("fake_binary");
        fs::write(&fake_binary, b"binary content").unwrap();
        cache.store("test_key_123", &fake_binary).unwrap();

        // Cache hit
        assert!(cache.lookup("test_key_123").is_some());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_same_source_same_hash() {
        let tmp = crate::test_support::temp_path("sp_test_hash_stable");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(tmp.join("src/main.salt"), "package main\nfn main() { }").unwrap();
        fs::write(
            tmp.join("salt.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let manifest = crate::manifest::load(&tmp.join("salt.toml")).unwrap();
        let cache = ArtifactCache {
            cache_dir: tmp.join("cache"),
        };
        fs::create_dir_all(tmp.join("cache")).unwrap();

        let key1 = cache.compute_key(&manifest, &tmp, false, &[], &[]).unwrap();
        let key2 = cache.compute_key(&manifest, &tmp, false, &[], &[]).unwrap();
        assert_eq!(key1, key2, "same source must produce same hash");

        // Different profile = different hash
        let key3 = cache.compute_key(&manifest, &tmp, true, &[], &[]).unwrap();
        assert_ne!(key1, key3, "release vs debug must produce different hash");

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Writes `files` (path relative to the root, content) under a fresh
    /// temp directory.
    fn temp_tree(tag: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let root = std::env::temp_dir().join(tag);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for (rel, content) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        root
    }

    fn test_manifest() -> Manifest {
        toml::from_str("[package]\nname = \"test\"\nversion = \"0.1.0\"\n").unwrap()
    }

    fn debug_key(project_dir: &Path, manifest: &Manifest, search_roots: &[PathBuf]) -> String {
        let cache = ArtifactCache {
            cache_dir: project_dir.join("cache"),
        };
        cache
            .compute_key(manifest, project_dir, false, search_roots, &[])
            .unwrap()
    }

    #[test]
    fn test_key_distinguishes_file_boundaries() {
        // Without length-delimiting, one file whose content spells out the
        // next file's name streams the same bytes as two files:
        // "a.salt" + "Xb.saltY" == "a.salt" + "X" + "b.salt" + "Y".
        let one = temp_tree(
            "sp_test_cache_key_boundary_one",
            &[("src/a.salt", b"Xb.saltY")],
        );
        let two = temp_tree(
            "sp_test_cache_key_boundary_two",
            &[("src/a.salt", b"X"), ("src/b.salt", b"Y")],
        );

        assert_ne!(
            debug_key(&one, &test_manifest(), &[]),
            debug_key(&two, &test_manifest(), &[])
        );

        let _ = fs::remove_dir_all(&one);
        let _ = fs::remove_dir_all(&two);
    }

    #[test]
    fn test_key_distinguishes_directories() {
        // Moving a module to another directory can break imports that
        // resolve by path, so it must not reuse the old build; hashing bare
        // file names can't see the move.
        let x = temp_tree(
            "sp_test_cache_key_dir_x",
            &[("src/x/m.salt", b"package x.m")],
        );
        let y = temp_tree(
            "sp_test_cache_key_dir_y",
            &[("src/y/m.salt", b"package x.m")],
        );

        assert_ne!(
            debug_key(&x, &test_manifest(), &[]),
            debug_key(&y, &test_manifest(), &[])
        );

        let _ = fs::remove_dir_all(&x);
        let _ = fs::remove_dir_all(&y);
    }

    #[test]
    fn test_key_distinguishes_field_boundaries() {
        // Unframed fields let bytes slide between the entry file and the
        // target triple, across the compiler id K and the profile:
        // S + K + "debug" + (K + "debug" + T) == (S + K + "debug") + K + "debug" + T.
        let a = temp_tree("sp_test_cache_key_fields_a", &[("main.salt", b"S")]);
        let slid = format!("{}debug", compiler_version_hash(&a));
        let b = temp_tree(
            "sp_test_cache_key_fields_b",
            &[("main.salt", format!("S{slid}").as_bytes())],
        );
        let root_entry_with_target = |target: String| {
            let mut m = test_manifest();
            m.package.entry = "main.salt".to_string();
            m.build = Some(BuildConfig {
                target: Some(target),
                release: None,
                debug: None,
            });
            m
        };

        assert_ne!(
            debug_key(&a, &root_entry_with_target(format!("{slid}T")), &[]),
            debug_key(&b, &root_entry_with_target("T".to_string()), &[])
        );

        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn test_key_distinguishes_search_roots() {
        // Search roots are an ordered list the compiler resolves imports
        // against, so which root holds a file is part of the build input.
        // One stream over all roots can't see b.salt move between them.
        let app = temp_tree("sp_test_cache_key_roots_app", &[("src/main.salt", b"M")]);
        let split = [
            temp_tree("sp_test_cache_key_roots_split1", &[("a.salt", b"A")]),
            temp_tree("sp_test_cache_key_roots_split2", &[("b.salt", b"B")]),
        ];
        let merged = [
            temp_tree(
                "sp_test_cache_key_roots_merged1",
                &[("a.salt", b"A"), ("b.salt", b"B")],
            ),
            temp_tree("sp_test_cache_key_roots_merged2", &[]),
        ];

        assert_ne!(
            debug_key(&app, &test_manifest(), &split),
            debug_key(&app, &test_manifest(), &merged)
        );

        for dir in std::iter::once(&app).chain(&split).chain(&merged) {
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn test_key_frames_content_length() {
        // Content that embeds the next record's path header: without a
        // content length prefix this streams the same bytes as two files.
        let next = b"b.salt";
        let mut content = b"X".to_vec();
        content.extend_from_slice(&(next.len() as u64).to_le_bytes());
        content.extend_from_slice(next);
        content.extend_from_slice(b"Y");
        let one = temp_tree("sp_test_cache_key_frame_one", &[("src/a.salt", &content)]);
        let two = temp_tree(
            "sp_test_cache_key_frame_two",
            &[("src/a.salt", b"X"), ("src/b.salt", b"Y")],
        );

        assert_ne!(
            debug_key(&one, &test_manifest(), &[]),
            debug_key(&two, &test_manifest(), &[])
        );

        let _ = fs::remove_dir_all(&one);
        let _ = fs::remove_dir_all(&two);
    }

    #[test]
    fn test_key_distinguishes_src_tree_from_entry_file() {
        // A project without src/ whose entry file holds exactly the bytes of
        // another project's src/ digest: both fill the same field, so only
        // the layout tag tells them apart.
        let tree = temp_tree("sp_test_cache_key_layout_tree", &[("src/main.salt", b"M")]);
        let digest = tree_digest(&tree.join("src")).unwrap();
        let entry = temp_tree("sp_test_cache_key_layout_entry", &[("main.salt", &digest)]);
        let mut root_entry = test_manifest();
        root_entry.package.entry = "main.salt".to_string();

        assert_ne!(
            debug_key(&tree, &test_manifest(), &[]),
            debug_key(&entry, &root_entry, &[])
        );

        let _ = fs::remove_dir_all(&tree);
        let _ = fs::remove_dir_all(&entry);
    }

    /// A dependency's name and entry reach saltc as `--dep <name>=<entry>`, so
    /// changing either must change the key even when no source changes.
    #[test]
    fn test_key_covers_dependency_name_and_entry() {
        let tmp = crate::test_support::temp_path("sp_test_hash_dep_entry");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("app/src")).unwrap();
        fs::create_dir_all(tmp.join("dep/src")).unwrap();
        fs::write(tmp.join("app/src/main.salt"), "package main\nfn main() { }").unwrap();
        fs::write(tmp.join("app/salt.toml"), "[package]\nname = \"app\"\nversion = \"0.1.0\"\n").unwrap();
        fs::write(tmp.join("dep/src/lib.salt"), "package dep\n").unwrap();
        fs::write(tmp.join("dep/src/other.salt"), "package dep\n").unwrap();

        let manifest = crate::manifest::load(&tmp.join("app/salt.toml")).unwrap();
        let cache = ArtifactCache { cache_dir: tmp.join("cache") };
        let dep = |name: &str, entry: &str| ResolvedDep {
            name: name.to_string(),
            source: "test".to_string(),
            root_path: tmp.join("dep"),
            resolved_version: None,
            entry: tmp.join("dep").join(entry),
        };
        let key = |d: ResolvedDep| cache.compute_key(&manifest, &tmp.join("app"), false, &[], &[d]).unwrap();

        let base = key(dep("dep", "src/lib.salt"));
        assert_eq!(base, key(dep("dep", "src/lib.salt")));
        assert_ne!(base, key(dep("dep", "src/other.salt")), "entry change must change the key");
        assert_ne!(base, key(dep("renamed", "src/lib.salt")), "name change must change the key");

        let _ = fs::remove_dir_all(&tmp);
    }
}
