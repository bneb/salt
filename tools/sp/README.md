# 🧂 sp — Salt Packaging

> *Two keystrokes. Zero friction.*

The Salt package manager. Content-addressed caching, cross-package Z3 contract verification, and Cargo-like ergonomics.

## Quick Start

```bash
# Install
cargo install --path .

# Create a project
sp new my_app && cd my_app

# Build and run
sp run

# Run tests
sp test
```

## Commands

| Command | Description |
|---------|-------------|
| `sp new <name>` | Create a new Salt project |
| `sp build` | Compile the project (with caching) |
| `sp run` | Build and run |
| `sp test` | Run tests in `tests/` |
| `sp check` | Verify Z3 contracts without building |
| `sp clean` | Remove build artifacts |
| `sp add <dep>` | Add a dependency to `salt.toml` |
| `sp fetch` | Download dependencies without building |
| `sp publish` | Package the project into `~/.salt/publish/` for use as a version dependency |

## Architecture

```
sp/src/
├── main.rs       # CLI entry point (clap)
├── manifest.rs   # salt.toml parser (serde + toml)
├── resolver.rs   # Dependency resolver + search root construction
├── compiler.rs   # salt-front orchestration
├── cache.rs      # Content-addressed artifact cache (~/.salt/cache/)
├── lockfile.rs   # salt.lock generation
├── publish.rs    # Local packaging (~/.salt/publish/) and extraction
└── semver.rs     # Version constraints for version dependencies
```

### Key Design Decisions

- **Content-addressed cache**: `sha256(source + compiler + profile + target + deps_hash)` → instant no-op builds
- **Transitive cache keys**: dependency hashes bubble up to prevent ABI mismatch from stale artifacts
- **Thin orchestration**: sp invokes `salt-front` and passes dependency search roots in `SALT_SEARCH_ROOTS`, never touching compiler internals. `salt-front` doesn't read that variable yet (see Status)
- **Non-destructive editing**: `sp add` uses `toml_edit` to preserve comments and formatting
- **Cross-package Z3**: contract manifests include AST stubs for types referenced in `requires`/`ensures`

## Status

Phase 1 (Foundation) is implemented:
- ✅ `sp new` — project scaffolding (binary + library templates)
- ✅ `sp build` — compilation with content-addressed caching
- ✅ `sp run` / `sp test` / `sp check` / `sp clean`
- ✅ `sp add` — non-destructive manifest editing
- ✅ Path dependency resolution with transitive support
- ✅ Version dependencies resolve to the highest matching version published with `sp publish`. A package's version is chosen at its first visit, from that requirement and the root manifest's; a later requirement that rejects it is a conflict error. sp doesn't backtrack, so pin versions in the root manifest to settle a conflict
- ✅ `sp publish` — packages a project into `~/.salt/publish/`
- ✅ `sp build` writes `salt.lock` (path dependencies aren't locked)

Planned:
- 🚧 Importing from dependencies: `salt-front` ignores the `SALT_SEARCH_ROOTS` that sp passes, so `use <dep>.…` fails to compile for path and version dependencies alike
- 🚧 PubGrub version resolution
- 🚧 Registry protocol (`registry.salt-lang.org`)
- 🚧 Package signing (Ed25519) and contract manifest extraction
- 🚧 Workspace support

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `sp build` fails with "salt-front not found" | Compiler binary not on PATH | `cd ../../salt-front && cargo build --release` then retry |
| `ld: library not found for -lz3` | Z3 not installed or not on library path | `brew install z3 && export DYLD_LIBRARY_PATH=/opt/homebrew/lib` |
| `sp run` segfaults | Missing `DYLD_LIBRARY_PATH` at runtime | `DYLD_LIBRARY_PATH=/opt/homebrew/lib sp run` |
| `sp build` fails with `no published versions found for 'X'` | `X` is a version dependency (for example from `sp add X`) that isn't in `~/.salt/publish/` | Run `sp publish` in X's directory. Importing it still won't compile (see Status) |
| `could not find imported module 'X.…'` when importing a dependency | `salt-front` doesn't read the search roots sp passes | Not supported yet (see Status) |
