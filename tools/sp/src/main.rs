#![allow(clippy::type_complexity)]
//! sp — Salt Packaging
//!
//! Two keystrokes. Zero friction.
//!
//! Usage:
//!   sp new <name>   — Create a new Salt project
//!   sp build        — Compile the project
//!   sp run          — Compile and run the project
//!   sp test         — Compile and run tests
//!   sp check        — Verify contracts without building
//!   sp clean        — Remove build artifacts
//!   sp add <dep>    — Add a dependency (future: registry)
//!   sp fetch        — Download dependencies without building
//!   sp publish      — Package the project for distribution

mod manifest;
mod resolver;
mod compiler;
mod cache;
mod semver;
mod publish;
mod lockfile;
#[cfg(test)]
mod test_support;

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "sp",
    version = "0.1.0",
    about = "🧂 sp — Salt Packaging. Two keystrokes. Zero friction.",
    long_about = "The Salt package manager.\n\nBuilt on three pillars: Fast Enough, Supremely Ergonomic, Formally Verified."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new Salt project
    New {
        /// Project name
        name: String,

        /// Use a library template instead of a binary
        #[arg(long)]
        lib: bool,
    },

    /// Compile the project
    Build {
        /// Path to project directory (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Build in release mode (O3 + verification)
        #[arg(long)]
        release: bool,

        /// Build a specific package in a workspace
        #[arg(short, long)]
        package: Option<String>,
    },

    /// Compile and run the project
    Run {
        /// Path to project directory
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Build in release mode
        #[arg(long)]
        release: bool,

        /// Arguments to pass to the program
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run tests
    Test {
        /// Path to project directory
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Filter tests by name
        #[arg(long)]
        filter: Option<String>,
    },

    /// Verify contracts without building
    Check {
        /// Path to project directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Remove build artifacts
    Clean {
        /// Path to project directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Add a dependency
    Add {
        /// Dependency name (optionally with @version)
        dep: String,

        /// Path to project directory
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Add as a dev-dependency
        #[arg(long)]
        dev: bool,
    },

    /// Download dependencies without building
    Fetch {
        /// Path to project directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },

    /// Package the project for distribution
    Publish {
        /// Path to project directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::New { name, lib } => cmd_new(&name, lib),
        Commands::Build { path, release, package } => cmd_build(&path, release, package.as_deref()),
        Commands::Run { path, release, args } => cmd_run(&path, release, &args),
        Commands::Test { path, filter } => cmd_test(&path, filter.as_deref()),
        Commands::Check { path } => cmd_check(&path),
        Commands::Clean { path } => cmd_clean(&path),
        Commands::Add { dep, path, dev } => cmd_add(&dep, &path, dev),
        Commands::Fetch { path } => cmd_fetch(&path),
        Commands::Publish { path } => cmd_publish(&path),
    };

    if let Err(e) = result {
        eprintln!("\x1b[1;31merror\x1b[0m: {}", e);
        std::process::exit(1);
    }
}

// ─── sp new ──────────────────────────────────────────────────────────────────

fn generate_manifest(project_name: &str, entry_path: &str) -> String {
    format!("[package]\nname = \"{project_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\nentry = \"{entry_path}\"\n")
}

fn generate_entry_source(project_name: &str, lib: bool) -> String {
    if lib {
        format!("package {project_name}\n\n/// Add two numbers.\npub fn add(a: i32, b: i32) -> i32 {{\n    return a + b;\n}}\n")
    } else {
        format!("package main\n\nfn main() -> i32 {{\n    println(\"Hello from {project_name}!\");\n    return 0;\n}}\n")
    }
}

fn generate_test_source(project_name: &str, lib: bool) -> String {
    if lib {
        format!("package test\n\nuse {project_name}.add\n\nfn main() -> i32 {{\n    let result = add(2, 3);\n    if result == 5 {{\n        println(\"PASS: add(2, 3) == 5\");\n    }} else {{\n        println(\"FAIL: add(2, 3) != 5\");\n        return 1;\n    }}\n    return 0;\n}}\n")
    } else {
        "package test\n\nfn main() -> i32 {\n    println(\"PASS: smoke test\");\n    return 0;\n}\n".to_string()
    }
}

fn cmd_new(name: &str, lib: bool) -> Result<(), String> {
    let project_dir = PathBuf::from(name);
    if project_dir.exists() { return Err(format!("directory '{}' already exists", name)); }

    let project_name = project_dir.file_name().ok_or_else(|| "invalid project name".to_string())?.to_string_lossy().to_string();

    std::fs::create_dir_all(project_dir.join("src")).map_err(|e| format!("failed to create src/: {}", e))?;
    std::fs::create_dir_all(project_dir.join("tests")).map_err(|e| format!("failed to create tests/: {}", e))?;

    let entry_path = if lib { "src/lib.salt" } else { "src/main.salt" };
    std::fs::write(project_dir.join("salt.toml"), generate_manifest(&project_name, entry_path)).map_err(|e| format!("failed to write salt.toml: {}", e))?;
    std::fs::write(project_dir.join(entry_path), generate_entry_source(&project_name, lib)).map_err(|e| format!("failed to write {}: {}", entry_path, e))?;

    std::fs::write(project_dir.join("tests/test_smoke.salt"), generate_test_source(&project_name, lib)).map_err(|e| format!("failed to write test: {}", e))?;
    std::fs::write(project_dir.join(".gitignore"), "target/\n*.o\n*.ll\n*.mlir\n").map_err(|e| format!("failed to write .gitignore: {}", e))?;

    println!("✨ Created project '{}'\n", project_name);
    println!("   {}/", name);
    println!("   ├── salt.toml");
    println!("   ├── src/");
    println!("   │   └── {}", if lib { "lib.salt" } else { "main.salt" });
    println!("   ├── tests/");
    println!("   │   └── test_smoke.salt");
    println!("   └── .gitignore");
    println!();
    if !lib {
        println!("   Run it: cd {} && sp run", name);
    }

    Ok(())
}

// ─── sp build ────────────────────────────────────────────────────────────────

fn cmd_build(path: &Path, release: bool, _package: Option<&str>) -> Result<(), String> {
    let start = Instant::now();
    let manifest_path = path.join("salt.toml");
    let manifest = manifest::load(&manifest_path)?;

    let mode_str = if release { "release" } else { "debug" };
    println!(
        "📦 Building \x1b[1m{}\x1b[0m v{} [{}]",
        manifest.package.name, manifest.package.version, mode_str
    );

    // Resolve dependencies — collect search roots for the compiler
    let (build_order, search_roots, resolved) = resolver::resolve(&manifest, path)?;

    let dep_count = manifest.dependencies.len();
    if dep_count > 0 {
        println!("   {} dependency(ies) resolved", dep_count);
    }

    // Before the cache check, so a cache hit still (re)creates a missing
    // salt.lock; output is deterministic, so an unchanged tree rewrites it
    // byte-identically. A failure only warns (like cache.store below): the
    // build doesn't depend on it.
    match write_lockfile(&manifest, path, &resolved) {
        Ok(()) => println!("   🔒 Wrote salt.lock"),
        Err(e) => eprintln!("\x1b[1;33mwarning\x1b[0m: salt.lock not written: {}", e),
    }

    // Check cache
    let cache = cache::ArtifactCache::new()?;
    let cache_key = cache.compute_key(&manifest, path, release, &search_roots, &resolved)?;

    if let Some(cached) = cache.lookup(&cache_key) {
        let elapsed = start.elapsed();
        println!(
            "⚡ \x1b[1;32mCached\x1b[0m {} ({}ms)",
            cached.display(),
            elapsed.as_millis()
        );
        return Ok(());
    }

    // Compile via salt-front with search roots
    println!("   🔨 Compiling {} module(s)...", build_order.len());
    let output = compiler::build(&manifest, path, release, &search_roots, &resolved)?;

    // Store in cache
    if let Ok(ref out) = Ok::<_, String>(output.clone()) {
        let _ = cache.store(&cache_key, out);
    }

    let elapsed = start.elapsed();
    println!(
        "✅ Built \x1b[1m{}\x1b[0m in {:.1}s",
        output.display(),
        elapsed.as_secs_f64()
    );

    Ok(())
}

/// Generate and persist salt.lock: the root package plus the resolved
/// version and content hash of each version dependency. Nothing reads it
/// back yet.
fn write_lockfile(
    manifest: &manifest::Manifest,
    project_dir: &Path,
    resolved: &[resolver::ResolvedDep],
) -> Result<(), String> {
    let lockfile = lockfile::generate(manifest, resolved)?;
    lockfile.save(&project_dir.join("salt.lock"))
}

// ─── sp run ──────────────────────────────────────────────────────────────────

fn cmd_run(path: &Path, release: bool, args: &[String]) -> Result<(), String> {
    cmd_build(path, release, None)?;

    let manifest_path = path.join("salt.toml");
    let manifest = manifest::load(&manifest_path)?;

    let binary = compiler::output_path(&manifest, path, release);
    println!("🧂 Running \x1b[1m{}\x1b[0m\n", manifest.package.name);

    let status = std::process::Command::new(&binary)
        .args(args)
        .env("DYLD_LIBRARY_PATH", "/opt/homebrew/lib")
        .status()
        .map_err(|e| format!("failed to run: {}", e))?;

    if !status.success() {
        return Err(format!("process exited with {}", status));
    }

    Ok(())
}

// ─── sp test ─────────────────────────────────────────────────────────────────

fn find_test_files(path: &Path, filter: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let test_dir = path.join("tests");
    if !test_dir.exists() { return Ok(Vec::new()); }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&test_dir)
        .map_err(|e| format!("failed to read tests/: {}", e))?
        .filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "salt")).collect();
    if let Some(f) = filter {
        files.retain(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().contains(f)));
    }
    files.sort();
    Ok(files)
}

fn cmd_test(path: &Path, filter: Option<&str>) -> Result<(), String> {
    let start = Instant::now();
    let manifest = manifest::load(&path.join("salt.toml"))?;

    let test_files = find_test_files(path, filter)?;
    if test_files.is_empty() {
        println!("No tests found");
        return Ok(());
    }

    println!("🧪 Running {} test(s) for \x1b[1m{}\x1b[0m\n", test_files.len(), manifest.package.name);

    let mut passed = 0; let mut failed = 0;
    for test_file in &test_files {
        let name = test_file.file_stem().unwrap().to_string_lossy();
        print!("   {} ... ", name);
        match compiler::run_test(test_file, path) {
            Ok(_) => { println!("\x1b[32m✓ pass\x1b[0m"); passed += 1; }
            Err(e) => { println!("\x1b[31m✗ FAIL\x1b[0m"); eprintln!("     {}", e); failed += 1; }
        }
    }

    println!("\n   Result: {} passed, {} failed ({:.1}s)", passed, failed, start.elapsed().as_secs_f64());
    if failed > 0 { Err(format!("{} test(s) failed", failed)) } else { Ok(()) }
}

// ─── sp check ────────────────────────────────────────────────────────────────

fn cmd_check(path: &Path) -> Result<(), String> {
    let start = Instant::now();
    let manifest_path = path.join("salt.toml");
    let manifest = manifest::load(&manifest_path)?;

    println!(
        "🔍 Checking \x1b[1m{}\x1b[0m v{}",
        manifest.package.name, manifest.package.version
    );

    // Resolve deps and compile with --verify flag
    let (_build_order, search_roots, resolved) = resolver::resolve(&manifest, path)?;
    compiler::check(&manifest, path, &search_roots, &resolved)?;

    let elapsed = start.elapsed();
    println!(
        "✅ All contracts verified ({:.1}s)",
        elapsed.as_secs_f64()
    );

    Ok(())
}

// ─── sp clean ────────────────────────────────────────────────────────────────

fn cmd_clean(path: &Path) -> Result<(), String> {
    let target_dir = path.join("target");

    if !target_dir.exists() {
        println!("   Nothing to clean");
        return Ok(());
    }

    let size = dir_size(&target_dir);
    std::fs::remove_dir_all(&target_dir)
        .map_err(|e| format!("failed to remove target/: {}", e))?;

    println!("🧹 Removed build artifacts ({:.1} MB)", size as f64 / 1_048_576.0);

    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let p = e.path();
                    if p.is_dir() {
                        dir_size(&p)
                    } else {
                        e.metadata().map(|m| m.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

// ─── sp add ──────────────────────────────────────────────────────────────────

fn cmd_add(dep: &str, path: &Path, dev: bool) -> Result<(), String> {
    let manifest_path = path.join("salt.toml");
    if !manifest_path.exists() {
        return Err("no salt.toml found. Run `sp new <name>` to create a project.".into());
    }

    // Parse dep@version syntax
    let (name, version) = if let Some(at) = dep.find('@') {
        (&dep[..at], &dep[at + 1..])
    } else {
        (dep, "*")
    };

    // Read and modify manifest using toml_edit for non-destructive editing
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("failed to read salt.toml: {}", e))?;

    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("failed to parse salt.toml: {}", e))?;

    let table_name = if dev { "dev-dependencies" } else { "dependencies" };

    // Ensure the table exists
    if doc.get(table_name).is_none() {
        doc[table_name] = toml_edit::Item::Table(toml_edit::Table::new());
    }

    // Add the dependency
    doc[table_name][name] = toml_edit::value(version);

    std::fs::write(&manifest_path, doc.to_string())
        .map_err(|e| format!("failed to write salt.toml: {}", e))?;

    let section = if dev { "dev-dependencies" } else { "dependencies" };
    println!(
        "✨ Added \x1b[1m{}\x1b[0m {} to [{}]",
        name, version, section
    );

    // What `sp add` writes is a version dependency, which resolves only from
    // ~/.salt/publish. Dev-dependencies aren't resolved by anything yet.
    if !dev {
        match publish::find_published(name) {
            Ok(found) if found.is_empty() => eprintln!(
                "\x1b[1;33mwarning\x1b[0m: nothing named '{}' is published in ~/.salt/publish, \
                 so `sp build` will fail until it is.\n  \
                 Publish it with `sp publish` from its directory.",
                name
            ),
            Ok(_) => {}
            Err(e) => eprintln!(
                "\x1b[1;33mwarning\x1b[0m: couldn't check whether '{}' is published: {}",
                name, e
            ),
        }
    }

    Ok(())
}

// ─── sp fetch ────────────────────────────────────────────────────────────────

fn cmd_fetch(path: &Path) -> Result<(), String> {
    let manifest_path = path.join("salt.toml");
    let manifest = manifest::load(&manifest_path)?;

    let dep_count = manifest.dependencies.len();
    if dep_count == 0 {
        println!("   No dependencies to fetch");
        return Ok(());
    }

    let (_build_order, _search_roots, _resolved) = resolver::resolve(&manifest, path)?;

    println!(
        "📥 Fetched {} package(s)",
        dep_count
    );

    Ok(())
}

// ─── sp publish ──────────────────────────────────────────────────────────────

fn cmd_publish(path: &Path) -> Result<(), String> {
    let manifest_path = path.join("salt.toml");
    let manifest = manifest::load(&manifest_path)?;
    publish::publish(&manifest, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_write_lockfile_creates_salt_lock() {
        let tmp = crate::test_support::temp_path("sp_test_write_lockfile");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(
            tmp.join("salt.toml"),
            "[package]\nname = \"locktest\"\nversion = \"0.2.0\"\n",
        )
        .unwrap();
        fs::write(
            tmp.join("src/main.salt"),
            "package main\nfn main() -> i32 { return 0; }\n",
        )
        .unwrap();

        let manifest = manifest::load(&tmp.join("salt.toml")).unwrap();
        let (_build_order, _search_roots, resolved) = resolver::resolve(&manifest, &tmp).unwrap();

        write_lockfile(&manifest, &tmp, &resolved).unwrap();

        let lock_path = tmp.join("salt.lock");
        assert!(lock_path.exists(), "expected salt.lock to be written");

        let loaded = lockfile::Lockfile::load(&lock_path).unwrap();
        let pkg = loaded
            .packages
            .get("locktest")
            .expect("main package should be locked");
        assert_eq!(pkg.version, "0.2.0");
        assert_eq!(pkg.hash, None, "the root package isn't content-hashed");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_cmd_publish_creates_archive() {
        let tmp_home = crate::test_support::temp_path("sp_test_cmd_publish_home");
        let project_dir = crate::test_support::temp_path("sp_test_cmd_publish_project");
        let _ = fs::remove_dir_all(&tmp_home);
        let _ = fs::remove_dir_all(&project_dir);
        fs::create_dir_all(&tmp_home).unwrap();
        fs::create_dir_all(project_dir.join("src")).unwrap();
        fs::write(
            project_dir.join("salt.toml"),
            "[package]\nname = \"pubtest\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(
            project_dir.join("src/main.salt"),
            "package main\nfn main() -> i32 { return 0; }\n",
        )
        .unwrap();

        let guard = crate::test_support::HomeGuard::new(&tmp_home);

        cmd_publish(&project_dir).expect("cmd_publish should succeed");

        let archive = tmp_home.join(".salt/publish/pubtest-0.1.0.tar.gz");
        assert!(
            archive.exists(),
            "expected archive at {}",
            archive.display()
        );

        drop(guard);
        let _ = fs::remove_dir_all(&tmp_home);
        let _ = fs::remove_dir_all(&project_dir);
    }
}
