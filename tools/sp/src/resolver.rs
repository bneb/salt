//! Dependency Resolver — resolves the dependency graph and constructs search roots
//!
//! Handles path dependencies and version dependencies resolved from the
//! local publish directory (~/.salt/publish/).
//!
//! Resolution is greedy and never backtracks. The graph is walked
//! depth-first in sorted order, and each package is selected at its first
//! visit from the requirements known then: the visiting one, and the root
//! manifest's requirement on the package, which is known from the start, so
//! a version pinned there always holds. A path requirement selects its
//! directory; otherwise the highest published version they all accept is
//! selected. Every later requirement must accept that selection, or
//! resolution fails with a conflict.

use crate::manifest::{Manifest, Dependency};
use crate::semver::{self, Constraint, Version};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Resolved dependency information.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ResolvedDep {
    pub name: String,
    pub source: String,
    pub root_path: PathBuf,
    /// The pinned version after resolution (None for path deps).
    pub resolved_version: Option<String>,
}

/// Resolve the build order and search roots for a project.
///
/// Returns:
///   - build_order: list of .salt files in compilation order (deps first)
///   - search_roots: list of paths for the compiler's `--roots` flag
///   - resolved_deps: resolved dependency metadata (for lockfile generation)
pub fn resolve(
    manifest: &Manifest,
    project_dir: &Path,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>, Vec<ResolvedDep>), String> {
    let mut build_order = Vec::new();
    let mut search_roots = Vec::new();
    let mut resolved_deps = Vec::new();
    let mut resolver = Resolver::new(manifest, project_dir)?;

    // Resolve each dependency
    for dep_name in manifest.dependencies.keys() {
        let resolved = resolver.resolve_root_dep(dep_name)?;

        for r in &resolved {
            // Add the dep's src/ directory (or root) as a search root
            let src_dir = r.root_path.join("src");
            if src_dir.exists() {
                search_roots.push(src_dir);
            } else {
                search_roots.push(r.root_path.clone());
            }

            // Collect .salt files from the dependency
            let dep_files = collect_salt_files(&r.root_path)?;
            build_order.extend(dep_files);
        }

        // Track resolved dep metadata for lockfile
        resolved_deps.extend(resolved);
    }

    // Add the project's own source
    let src_dir = project_dir.join("src");
    if src_dir.exists() {
        search_roots.insert(0, src_dir.clone());
        let src_files = collect_salt_files(&src_dir)?;
        build_order.extend(src_files);
    } else {
        search_roots.insert(0, project_dir.to_path_buf());
        let entry = project_dir.join(&manifest.package.entry);
        if entry.exists() {
            build_order.push(entry);
        } else {
            return Err(format!(
                "entry point not found: {}",
                manifest.package.entry
            ));
        }
    }

    // Also add the stdlib root — search upward for the std/ symlink or directory
    if let Some(std_root) = find_stdlib(project_dir) {
        search_roots.push(std_root);
    }

    // Deduplicate while preserving order
    let mut seen = HashSet::new();
    build_order.retain(|f| {
        let canonical = f.canonicalize().unwrap_or_else(|_| f.clone());
        seen.insert(canonical)
    });

    Ok((build_order, search_roots, resolved_deps))
}

/// One package's requirement on another, from its salt.toml.
struct Requirement {
    /// The package that declares it, as "<name> <version>".
    dependent: String,
    /// The path or version constraint as written.
    text: String,
    kind: RequirementKind,
}

enum RequirementKind {
    /// The canonical directory of `{ path = "..." }`.
    Path(PathBuf),
    Version(Vec<Constraint>),
}

impl Requirement {
    fn new(name: &str, dep: &Dependency, base_dir: &Path, dependent: &str) -> Result<Self, String> {
        let (text, kind) = match dep {
            Dependency::Path { path, features } => {
                reject_features(name, features)?;
                let dep_dir = base_dir.join(path);
                if !dep_dir.exists() {
                    return Err(format!(
                        "dependency '{}' path not found: {}",
                        name,
                        dep_dir.display()
                    ));
                }
                (path, RequirementKind::Path(dep_dir.canonicalize().unwrap_or(dep_dir)))
            }

            Dependency::Version(version) => (version, parse_version_requirement(name, version)?),

            Dependency::Full { version, features } => {
                reject_features(name, features)?;
                (version, parse_version_requirement(name, version)?)
            }

            Dependency::Git { git, .. } => {
                return Err(format!(
                    "git dependency '{}' from '{}' requires git clone support.\n  \
                     Workaround: clone the repo manually and use a path dependency.\n  \
                     Git dependency support is planned for sp v0.3.0.",
                    name, git
                ));
            }
        };
        Ok(Requirement { dependent: dependent.to_string(), text: text.clone(), kind })
    }

    fn constraints(&self) -> Option<&[Constraint]> {
        match &self.kind {
            RequirementKind::Path(_) => None,
            RequirementKind::Version(constraints) => Some(constraints),
        }
    }

    fn accepts(&self, selection: &Selection) -> bool {
        match &self.kind {
            RequirementKind::Path(dir) => selection.from_path && selection.dir == *dir,
            // A package without a version satisfies only a requirement that
            // constrains nothing ("*").
            RequirementKind::Version(constraints) => match &selection.version {
                Some(version) => semver::satisfies(version, constraints),
                None => constraints.is_empty(),
            },
        }
    }

    /// The requirement on `name` in salt.toml syntax.
    fn describe(&self, name: &str) -> String {
        match &self.kind {
            RequirementKind::Path(dir) => {
                format!("{} = {{ path = \"{}\" }} ({})", name, self.text, dir.display())
            }
            RequirementKind::Version(_) => format!("{} = \"{}\"", name, self.text),
        }
    }
}

/// Feature flags on dependencies aren't implemented, and dropping requested
/// ones silently would build something other than what was asked for.
fn reject_features(name: &str, features: &[String]) -> Result<(), String> {
    if features.is_empty() {
        return Ok(());
    }
    Err(format!(
        "dependency '{}' requests features {:?}, but feature flags on \
         dependencies are not supported yet.\n  \
         To build without them, drop `features` from the '{}' dependency.",
        name, features, name
    ))
}

fn parse_version_requirement(name: &str, constraint: &str) -> Result<RequirementKind, String> {
    semver::parse_constraints(constraint)
        .map(RequirementKind::Version)
        .map_err(|e| format!("invalid version constraint for '{}': {}", name, e))
}

/// The version constraints of all `reqs` together: a version satisfies them
/// if every version requirement accepts it.
fn combined_constraints<'a>(reqs: impl IntoIterator<Item = &'a Requirement>) -> Vec<Constraint> {
    reqs.into_iter()
        .flat_map(|r| r.constraints().unwrap_or_default())
        .cloned()
        .collect()
}

/// The package a name resolved to, and the requirements it satisfies.
struct Selection {
    /// Canonical package root: a path dependency's directory, or the
    /// extracted copy of a published archive.
    dir: PathBuf,
    /// Whether a path requirement selected it.
    from_path: bool,
    /// The published version, or the one a path package's salt.toml
    /// declares (None if it has no salt.toml or the version doesn't parse).
    version: Option<Version>,
    required_by: Vec<Requirement>,
}

impl Selection {
    fn describe(&self, name: &str) -> String {
        match (&self.version, self.from_path) {
            (Some(version), false) => format!("{} {}", name, version),
            (Some(version), true) => format!("{} {} from {}", name, version, self.dir.display()),
            (None, _) => format!("{} from {} (no version declared)", name, self.dir.display()),
        }
    }
}

/// Walk state for one resolve().
struct Resolver {
    /// The root manifest's requirements not applied yet. Each is applied at
    /// its package's first visit, whichever package makes it.
    root_requirements: HashMap<String, Requirement>,
    root_manifest: PathBuf,
    selected: HashMap<String, Selection>,
}

impl Resolver {
    fn new(manifest: &Manifest, project_dir: &Path) -> Result<Self, String> {
        let root = format!("{} {}", manifest.package.name, manifest.package.version);
        let mut root_requirements = HashMap::new();
        for (name, dep) in &manifest.dependencies {
            root_requirements.insert(name.clone(), Requirement::new(name, dep, project_dir, &root)?);
        }
        Ok(Resolver {
            root_requirements,
            root_manifest: project_dir.join("salt.toml"),
            selected: HashMap::new(),
        })
    }

    /// Resolves one of the root manifest's dependencies. If another
    /// dependency reached it first, that visit already applied the root's
    /// requirement, and there is nothing left to do.
    fn resolve_root_dep(&mut self, name: &str) -> Result<Vec<ResolvedDep>, String> {
        match self.root_requirements.remove(name) {
            Some(req) => self.resolve_single(name, req),
            None => Ok(vec![]),
        }
    }

    /// Applies one requirement on `name`. The package's first visit selects
    /// it and resolves its own dependencies, returning them, then it, in
    /// build order. A later visit only checks that the requirement accepts
    /// the selection.
    fn resolve_single(&mut self, name: &str, req: Requirement) -> Result<Vec<ResolvedDep>, String> {
        if let Some(selection) = self.selected.get_mut(name) {
            if !req.accepts(selection) {
                let reqs: Vec<&Requirement> = selection.required_by.iter().chain([&req]).collect();
                return Err(conflict(name, &reqs, Some((&*selection, &req)), &self.root_manifest));
            }
            selection.required_by.push(req);
            return Ok(vec![]);
        }

        let mut reqs: Vec<Requirement> = self.root_requirements.remove(name).into_iter().collect();
        reqs.push(req);
        let (selection, resolved, manifest) = select(name, reqs, &self.root_manifest)?;
        let dir = selection.dir.clone();
        // Recorded before resolving its dependencies, which may lead back to it.
        self.selected.insert(name.to_string(), selection);

        let mut result = vec![];
        if let Some(manifest) = manifest {
            let dependent = format!("{} {}", name, manifest.package.version);
            for (dep_name, dep) in &manifest.dependencies {
                let dep_req = Requirement::new(dep_name, dep, &dir, &dependent)?;
                result.extend(self.resolve_single(dep_name, dep_req)?);
            }
        }
        result.push(resolved);
        Ok(result)
    }
}

/// Selects a package for `name` at its first visit: a path requirement's
/// directory, else the highest published version every requirement accepts.
/// Returns the selection, its entry for the lockfile, and its manifest.
fn select(
    name: &str,
    reqs: Vec<Requirement>,
    root_manifest: &Path,
) -> Result<(Selection, ResolvedDep, Option<Manifest>), String> {
    let path_req = reqs.iter().find_map(|r| match &r.kind {
        RequirementKind::Path(dir) => Some((r.text.as_str(), dir)),
        RequirementKind::Version(_) => None,
    });
    let (dir, published_version, source) = match path_req {
        Some((text, dir)) => (dir.clone(), None, format!("path:{}", text)),
        None => {
            let (version, dir) = select_published(name, &reqs, root_manifest)?;
            let source = format!("v{}", version);
            (dir, Some(version), source)
        }
    };
    let from_path = path_req.is_some();

    let manifest_path = dir.join("salt.toml");
    let manifest = if manifest_path.exists() {
        Some(crate::manifest::load(&manifest_path)?)
    } else {
        None
    };

    let resolved = ResolvedDep {
        name: name.to_string(),
        source,
        root_path: dir.clone(),
        resolved_version: published_version.as_ref().map(Version::to_string),
    };
    let version = published_version.or_else(|| {
        manifest
            .as_ref()
            .and_then(|m| semver::parse_version(&m.package.version).ok())
    });
    let selection = Selection { dir, from_path, version, required_by: vec![] };

    // A path requirement selects its directory whatever the other
    // requirement says, so that one must still accept it.
    if let Some(rejecting) = reqs.iter().find(|r| !r.accepts(&selection)) {
        let all: Vec<&Requirement> = reqs.iter().collect();
        return Err(conflict(name, &all, Some((&selection, rejecting)), root_manifest));
    }
    Ok((Selection { required_by: reqs, ..selection }, resolved, manifest))
}

/// The highest published version of `name` that every requirement accepts
/// (all of them version requirements), and the directory it's extracted to.
fn select_published(
    name: &str,
    reqs: &[Requirement],
    root_manifest: &Path,
) -> Result<(Version, PathBuf), String> {
    // Find published versions
    let published = crate::publish::find_published(name)?;
    if published.is_empty() {
        return Err(format!(
            "no published versions found for '{}'.\n  \
             Hint: publish the package first with `sp publish` from its directory.",
            name
        ));
    }

    // Pick the best matching version
    let versions: Vec<Version> = published.iter().map(|(v, _)| v.clone()).collect();
    let Some(best) = semver::best_match(&versions, &combined_constraints(reqs)) else {
        if let [req] = reqs {
            return Err(format!(
                "no published version of '{}' matches constraints '{}'.\n  \
                 Available versions: {}",
                name,
                req.text,
                versions
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let all: Vec<&Requirement> = reqs.iter().collect();
        return Err(conflict(name, &all, None, root_manifest));
    };

    let (_, archive) = published
        .iter()
        .find(|(v, _)| v == best)
        .expect("best_match picks one of the published versions");
    let dep_dir = crate::publish::extract_package(name, best, archive)?;
    Ok((best.clone(), dep_dir.canonicalize().unwrap_or(dep_dir)))
}

/// The error for requirements on `name` that no one package satisfies:
/// `rejected` is the selection and the requirement that rejects it, or None
/// if no published version satisfies them all. sp doesn't backtrack, so it
/// can't tell whether any solution exists, and mustn't imply there is none.
fn conflict(
    name: &str,
    reqs: &[&Requirement],
    rejected: Option<(&Selection, &Requirement)>,
    root_manifest: &Path,
) -> String {
    let mut msg = format!("conflicting requirements for '{}':", name);
    for req in reqs {
        msg.push_str(&format!("\n    {} requires {}", req.dependent, req.describe(name)));
    }

    // With only version requirements, a published version they all accept
    // can be suggested as a pin.
    let versions: Vec<Version> = if reqs.iter().all(|r| r.constraints().is_some()) {
        crate::publish::find_published(name)
            .map(|found| found.into_iter().map(|(v, _)| v).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let fit = semver::best_match(&versions, &combined_constraints(reqs.iter().copied()));

    match rejected {
        Some((selection, req)) => {
            msg.push_str(&format!(
                "\n  sp selected {}, which {} rejects.",
                selection.describe(name),
                req.dependent
            ));
            // Two directories: no version choice or pin helps; only editing
            // one of these path dependencies does.
            if selection.from_path && matches!(req.kind, RequirementKind::Path(_)) {
                msg.push_str(
                    "\n  A package can come from only one directory.\n  \
                     Hint: point these path dependencies at the same one.",
                );
                return msg;
            }
        }
        None => msg.push_str(&format!(
            "\n  No published version of '{}' satisfies all of them (available: {}).",
            name,
            versions.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ")
        )),
    }
    msg.push_str("\n  sp doesn't backtrack, so this doesn't mean no solution exists");
    match fit {
        Some(version) => msg.push_str(&format!(
            ": {} {} satisfies all of them.\n  Hint: pin it in the root manifest ({}): {} = \"={}\"",
            name,
            version,
            root_manifest.display(),
            name,
            version
        )),
        None => msg.push_str(&format!(
            ".\n  Hint: pin '{}', or the packages that require it, in the root manifest ({}) \
             so that their requirements agree.",
            name,
            root_manifest.display()
        )),
    }
    msg
}

/// Find the Salt stdlib by searching upward from the project directory.
fn find_stdlib(project_dir: &Path) -> Option<PathBuf> {
    let mut dir = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());

    loop {
        // Check for salt-front/std/ (the canonical stdlib location)
        let std_candidate = dir.join("salt-front").join("std");
        if std_candidate.exists() {
            return Some(std_candidate);
        }

        // Check for a std/ symlink or directory
        let std_direct = dir.join("std");
        if std_direct.exists() {
            return Some(std_direct);
        }

        if !dir.pop() {
            break;
        }
    }

    // Fallback: check SALT_REPO_ROOT environment variable
    if let Ok(repo_root) = std::env::var("SALT_REPO_ROOT") {
        let env_stdlib = PathBuf::from(&repo_root).join("salt-front").join("std");
        if env_stdlib.exists() {
            return Some(env_stdlib);
        }
        // Fallback for new repo layout: salt/std
        let env_stdlib_new = PathBuf::from(&repo_root).join("std");
        if env_stdlib_new.exists() {
            return Some(env_stdlib_new);
        }
    }

    // Fallback: check global ~/.salt/std
    if let Some(home) = std::env::var_os("HOME") {
        let global_stdlib = PathBuf::from(home).join(".salt").join("std");
        if global_stdlib.exists() {
            return Some(global_stdlib);
        }
    }

    None
}

/// Collect all .salt files in a directory recursively.
fn collect_salt_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();

    if !dir.exists() {
        return Ok(files);
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read {}: {}", dir.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("read error: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            // Skip target/ and hidden directories
            let name = path.file_name().unwrap().to_string_lossy();
            if name.starts_with('.') || name == "target" || name == "tests" {
                continue;
            }
            files.extend(collect_salt_files(&path)?);
        } else if path.extension().is_some_and(|e| e == "salt") {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_collect_salt_files() {
        let tmp = crate::test_support::temp_path("sp_test_collect");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(tmp.join("src/main.salt"), "package main").unwrap();
        fs::write(tmp.join("src/lib.salt"), "package lib").unwrap();
        fs::write(tmp.join("src/readme.md"), "# readme").unwrap();

        let files = collect_salt_files(&tmp.join("src")).unwrap();
        assert_eq!(files.len(), 2, "should find exactly 2 .salt files");
        assert!(files.iter().all(|f| f.extension().unwrap() == "salt"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_simple_project() {
        let tmp = crate::test_support::temp_path("sp_test_resolve");
        let _ = fs::remove_dir_all(&tmp);

        // Create a simple project
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(
            tmp.join("salt.toml"),
            r#"
[package]
name = "test_app"
version = "0.1.0"
"#,
        )
        .unwrap();
        fs::write(tmp.join("src/main.salt"), "package main\nfn main() -> i32 { return 0; }").unwrap();

        let manifest = crate::manifest::load(&tmp.join("salt.toml")).unwrap();
        let (build_order, search_roots, _resolved_deps) = resolve(&manifest, &tmp).unwrap();

        assert_eq!(build_order.len(), 1, "should have 1 source file");
        assert!(!search_roots.is_empty(), "should have at least 1 search root");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_rejects_dependency_features() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_features");
        let _ = fs::remove_dir_all(&tmp);

        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(
            tmp.join("salt.toml"),
            r#"
[package]
name = "test_app"
version = "0.1.0"

[dependencies]
json = { version = "1.0", features = ["streaming"] }
"#,
        )
        .unwrap();
        fs::write(tmp.join("src/main.salt"), "package main\nfn main() -> i32 { return 0; }").unwrap();

        let manifest = crate::manifest::load(&tmp.join("salt.toml")).unwrap();
        let err = resolve(&manifest, &tmp).unwrap_err();

        assert!(err.contains("'json'"), "error should name the dependency: {err}");
        assert!(err.contains("streaming"), "error should name the requested feature: {err}");
        assert!(err.contains("not supported"), "error should say features are unsupported: {err}");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_rejects_path_dependency_features() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_path_features");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        write_package(&tmp.join("helper"), "helper", "1.0.0", "");

        let manifest = app_with(&tmp, "helper = { path = \"../helper\", features = [\"fast\"] }\n");
        let err = resolve(&manifest, &tmp.join("app")).expect_err("the features must not be dropped silently");

        assert!(err.contains("'helper'"), "error should name the dependency: {err}");
        assert!(err.contains("fast"), "error should name the requested feature: {err}");
        assert!(err.contains("not supported"), "error should say features are unsupported: {err}");

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// App project under `root/app` with `deps` as its [dependencies] table.
    fn app_with(root: &Path, deps: &str) -> Manifest {
        let app = root.join("app");
        fs::create_dir_all(app.join("src")).unwrap();
        fs::write(
            app.join("salt.toml"),
            format!("[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\n{}", deps),
        )
        .unwrap();
        fs::write(app.join("src/main.salt"), "package main\nfn main() -> i32 { return 0; }\n").unwrap();
        crate::manifest::load(&app.join("salt.toml")).unwrap()
    }

    /// App project under `root/app` depending on one package by version.
    fn app_depending_on(root: &Path, dep: &str, constraint: &str) -> Manifest {
        app_with(root, &format!("{} = \"{}\"\n", dep, constraint))
    }

    /// Writes package `name` at `version` into `dir`, with `deps` as its
    /// [dependencies] table and a source file naming the version.
    fn write_package(dir: &Path, name: &str, version: &str, deps: &str) {
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.salt"), format!("package {}\n// version {}\n", name, version)).unwrap();
        fs::write(
            dir.join("salt.toml"),
            format!("[package]\nname = \"{}\"\nversion = \"{}\"\n\n[dependencies]\n{}", name, version, deps),
        )
        .unwrap();
    }

    /// Publishes `name` at `version` into $HOME/.salt/publish (see write_package).
    fn publish_package(root: &Path, name: &str, version: &str, deps: &str) {
        let dir = root.join("sources").join(format!("{}-{}", name, version));
        write_package(&dir, name, version, deps);
        crate::publish::publish(&crate::manifest::load(&dir.join("salt.toml")).unwrap(), &dir).unwrap();
    }

    /// "<name> <version>" per resolved dependency; "<name> path" for a path one.
    fn selected(resolved: &[ResolvedDep]) -> Vec<String> {
        resolved
            .iter()
            .map(|r| format!("{} {}", r.name, r.resolved_version.as_deref().unwrap_or("path")))
            .collect()
    }

    #[test]
    fn test_resolve_version_dep_errors_when_unpublished() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_unpublished");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("home")).unwrap();
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));

        let manifest = app_depending_on(&tmp, "nosuchpkg", "1.0");
        let err = resolve(&manifest, &tmp.join("app"))
            .expect_err("an unpublished version dependency must not resolve to nothing");
        assert!(err.contains("no published versions found for 'nosuchpkg'"), "{err}");

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_version_dep_round_trip() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_round_trip");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("home")).unwrap();
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));

        // Publish three versions into the isolated $HOME/.salt/publish, each
        // with distinct source so the test can tell which one was extracted.
        let lib = tmp.join("mylib");
        fs::create_dir_all(lib.join("src")).unwrap();
        let source = |version: &str| format!("package mylib\n// version {}\n", version);
        for version in ["0.3.0", "0.3.1", "0.4.0"] {
            fs::write(lib.join("src/lib.salt"), source(version)).unwrap();
            fs::write(
                lib.join("salt.toml"),
                format!("[package]\nname = \"mylib\"\nversion = \"{}\"\n", version),
            )
            .unwrap();
            let lib_manifest = crate::manifest::load(&lib.join("salt.toml")).unwrap();
            crate::publish::publish(&lib_manifest, &lib).unwrap();
        }

        // The constraint admits 0.3.x only; 0.4.0 is newer but must be skipped.
        let manifest = app_depending_on(&tmp, "mylib", ">=0.3.0, <0.4.0");
        let (_order, search_roots, resolved) = resolve(&manifest, &tmp.join("app")).unwrap();

        assert_eq!(resolved.len(), 1, "exactly one dependency should resolve: {resolved:?}");
        let dep = &resolved[0];
        assert_eq!(dep.name, "mylib");
        assert_eq!(dep.resolved_version.as_deref(), Some("0.3.1"), "highest version matching the constraint");
        assert_eq!(
            fs::read_to_string(dep.root_path.join("src/lib.salt")).unwrap(),
            source("0.3.1"),
            "the extracted package must be the version that was reported"
        );
        assert!(
            search_roots.iter().any(|r| r.starts_with(&dep.root_path)),
            "compiler search roots should include the extracted package: {search_roots:?}"
        );

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_version_dep_published_with_two_components() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_two_components");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));

        // Archived as two-0.1.tar.gz; the resolver normalizes the version to 0.1.0.
        publish_package(&tmp, "two", "0.1", "");

        let manifest = app_depending_on(&tmp, "two", "0.1");
        let (_order, _roots, resolved) =
            resolve(&manifest, &tmp.join("app")).expect("a package published as version \"0.1\" must resolve");
        assert_eq!(resolved.len(), 1, "{resolved:?}");
        assert_eq!(resolved[0].resolved_version.as_deref(), Some("0.1.0"));
        assert_eq!(
            fs::read_to_string(resolved[0].root_path.join("src/lib.salt")).unwrap(),
            "package two\n// version 0.1\n"
        );

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// a at 1.0.0, 1.5.0 and 2.0.0, and b 1.0.0, which requires a < 2.0.
    fn publish_a_and_b(root: &Path) {
        for version in ["1.0.0", "1.5.0", "2.0.0"] {
            publish_package(root, "a", version, "");
        }
        publish_package(root, "b", "1.0.0", "a = \"<2.0\"\n");
    }

    #[test]
    fn test_resolve_rejects_conflicting_versions() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_conflict");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        publish_a_and_b(&tmp);

        // The app's a >= 1.0 selects 2.0.0, which b's a < 2.0 then rejects.
        let manifest = app_with(&tmp, "a = \">=1.0\"\nb = \"1\"\n");
        let err = resolve(&manifest, &tmp.join("app")).expect_err("a 2.0.0 violates b's a < 2.0");

        assert!(err.contains("conflicting requirements for 'a'"), "{err}");
        assert!(err.contains("app 0.1.0 requires a = \">=1.0\""), "{err}");
        assert!(err.contains("b 1.0.0 requires a = \"<2.0\""), "{err}");
        assert!(err.contains("selected a 2.0.0, which b 1.0.0 rejects"), "{err}");
        // sp doesn't backtrack, so it must not call the conflict unsolvable.
        assert!(err.contains("a 1.5.0 satisfies all of them"), "{err}");
        assert!(err.contains("root manifest") && err.contains("a = \"=1.5.0\""), "{err}");

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_root_pin_settles_conflict() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_pin");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        publish_a_and_b(&tmp);

        // The pin the conflict error suggests.
        let manifest = app_with(&tmp, "a = \"=1.5.0\"\nb = \"1\"\n");
        let (_order, _roots, resolved) = resolve(&manifest, &tmp.join("app")).unwrap();
        assert_eq!(selected(&resolved), ["a 1.5.0", "b 1.0.0"]);

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_applies_root_requirement_at_first_visit() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_root_first");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        for version in ["1.0.0", "1.5.0", "2.0.0"] {
            publish_package(&tmp, "z", version, "");
        }
        publish_package(&tmp, "b", "1.0.0", "z = \">=1.0\"\n");

        // b reaches z before the app's own entry for z does, and b's
        // z >= 1.0 alone would select 2.0.0.
        let manifest = app_with(&tmp, "b = \"1\"\nz = \"=1.5.0\"\n");
        let (_order, _roots, resolved) =
            resolve(&manifest, &tmp.join("app")).expect("a pin in the root manifest must hold");
        assert_eq!(selected(&resolved), ["z 1.5.0", "b 1.0.0"]);

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_rejects_root_requirement_no_version_meets() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_root_unmet");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        for version in ["1.0.0", "1.5.0", "2.0.0"] {
            publish_package(&tmp, "z", version, "");
        }
        publish_package(&tmp, "y", "1.0.0", "z = \">=2.0\"\n");

        let manifest = app_with(&tmp, "y = \"1\"\nz = \"=1.5.0\"\n");
        let err = resolve(&manifest, &tmp.join("app")).expect_err("y's z >= 2.0 and the app's z = 1.5.0 can't both hold");

        assert!(err.contains("app 0.1.0 requires z = \"=1.5.0\""), "{err}");
        assert!(err.contains("y 1.0.0 requires z = \">=2.0\""), "{err}");
        assert!(
            err.contains("No published version of 'z' satisfies all of them (available: 1.0.0, 1.5.0, 2.0.0)"),
            "{err}"
        );
        assert!(err.contains("pin 'z', or the packages that require it"), "{err}");

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_selects_for_first_and_root_requirements_together() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_root_and_first");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        for version in ["1.0.0", "2.0.0"] {
            publish_package(&tmp, "json", version, "");
        }
        publish_package(&tmp, "http", "1.0.0", "json = \"^1\"\n");

        // `sp add json` writes json = "*". Selecting for the root's "*"
        // alone would pick 2.0.0, which http rejects.
        let manifest = app_with(&tmp, "http = \"1\"\njson = \"*\"\n");
        let (_order, _roots, resolved) = resolve(&manifest, &tmp.join("app")).unwrap();
        assert_eq!(selected(&resolved), ["json 1.0.0", "http 1.0.0"]);

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_rejects_one_package_at_two_paths() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_two_paths");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        write_package(&tmp.join("s1"), "shared", "1.0.0", "");
        write_package(&tmp.join("s2"), "shared", "1.0.0", "");
        write_package(&tmp.join("b"), "b", "0.1.0", "shared = { path = \"../s1\" }\n");
        write_package(&tmp.join("c"), "c", "0.1.0", "shared = { path = \"../s2\" }\n");

        let manifest = app_with(&tmp, "b = { path = \"../b\" }\nc = { path = \"../c\" }\n");
        let err = resolve(&manifest, &tmp.join("app")).expect_err("'shared' can't be two directories");

        assert!(err.contains("conflicting requirements for 'shared'"), "{err}");
        assert!(err.contains("b 0.1.0 requires shared = { path = \"../s1\" }"), "{err}");
        assert!(err.contains("c 0.1.0 requires shared = { path = \"../s2\" }"), "{err}");
        assert!(err.contains("point these path dependencies at the same one"), "{err}");

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_accepts_path_dependency_shared_by_dependents() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_shared_path");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        write_package(&tmp.join("shared"), "shared", "1.0.0", "");
        write_package(&tmp.join("b"), "b", "0.1.0", "shared = { path = \"../shared\" }\n");

        // One directory, reached from app/ and from b/.
        let manifest = app_with(&tmp, "b = { path = \"../b\" }\nshared = { path = \"../shared\" }\n");
        let (_order, _roots, resolved) = resolve(&manifest, &tmp.join("app")).unwrap();
        assert_eq!(selected(&resolved), ["shared path", "b path"]);

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_checks_version_requirement_against_path_package() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_path_version");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        publish_package(&tmp, "b", "1.0.0", "a = \"<2.0\"\n");
        write_package(&tmp.join("a"), "a", "2.0.0", "");

        let manifest = app_with(&tmp, "a = { path = \"../a\" }\nb = \"1\"\n");
        let err = resolve(&manifest, &tmp.join("app")).expect_err("the path package a is 2.0.0, and b requires a < 2.0");

        assert!(err.contains("app 0.1.0 requires a = { path = \"../a\" }"), "{err}");
        assert!(err.contains("b 1.0.0 requires a = \"<2.0\""), "{err}");
        assert!(err.contains("selected a 2.0.0 from"), "{err}");

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_rejects_path_requirement_on_published_package() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_path_after_version");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        publish_package(&tmp, "a", "1.0.0", "");
        write_package(&tmp.join("a"), "a", "1.0.0", "");
        write_package(&tmp.join("c"), "c", "0.1.0", "a = { path = \"../a\" }\n");

        let manifest = app_with(&tmp, "a = \"1\"\nc = { path = \"../c\" }\n");
        let err = resolve(&manifest, &tmp.join("app")).expect_err("c needs a from ../a, not the published a");

        assert!(err.contains("c 0.1.0 requires a = { path = \"../a\" }"), "{err}");
        assert!(err.contains("selected a 1.0.0, which c 0.1.0 rejects"), "{err}");

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_resolve_version_requirement_on_path_package_without_version() {
        let tmp = crate::test_support::temp_path("sp_test_resolve_path_no_version");
        let _ = fs::remove_dir_all(&tmp);
        let home = crate::test_support::HomeGuard::new(&tmp.join("home"));
        // No salt.toml, so no version.
        fs::create_dir_all(tmp.join("shared/src")).unwrap();
        fs::write(tmp.join("shared/src/lib.salt"), "package shared\n").unwrap();
        let manifest = app_with(&tmp, "b = { path = \"../b\" }\nshared = { path = \"../shared\" }\n");

        write_package(&tmp.join("b"), "b", "0.1.0", "shared = \"1\"\n");
        let err = resolve(&manifest, &tmp.join("app")).expect_err("a package without a version can't satisfy \"1\"");
        assert!(err.contains("b 0.1.0 requires shared = \"1\""), "{err}");
        assert!(err.contains("no version"), "{err}");

        write_package(&tmp.join("b"), "b", "0.1.0", "shared = \"*\"\n");
        let (_order, _roots, resolved) = resolve(&manifest, &tmp.join("app")).expect("\"*\" accepts any package");
        assert_eq!(selected(&resolved), ["shared path", "b path"]);

        drop(home);
        let _ = fs::remove_dir_all(&tmp);
    }
}
