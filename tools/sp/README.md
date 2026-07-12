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

# Add a dependency
sp add json

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

## Architecture

```
sp/src/
├── main.rs       # CLI entry point (clap)
├── manifest.rs   # salt.toml parser (serde + toml)
├── resolver.rs   # Dependency resolver + search root construction
├── compiler.rs   # salt-front orchestration (--roots injection)
└── cache.rs      # Content-addressed artifact cache (~/.salt/cache/)
```

### Key Design Decisions

- **Content-addressed cache**: `sha256(source + compiler + target + features + deps_hash)` → instant no-op builds
- **Transitive cache keys**: dependency hashes bubble up to prevent ABI mismatch from stale artifacts
- **Thin orchestration**: sp configures `salt-front` via `--roots`, never touching compiler internals
- **Non-destructive editing**: `sp add` uses `toml_edit` to preserve comments and formatting
- **Cross-package Z3**: contract manifests include AST stubs for types referenced in `requires`/`ensures`

## Status

Phase 1 (Foundation) is implemented:
- ✅ `sp new` — project scaffolding (binary + library templates)
- ✅ `sp build` — compilation with content-addressed caching
- ✅ `sp run` / `sp test` / `sp check` / `sp clean`
- ✅ `sp add` — non-destructive manifest editing
- ✅ Path dependency resolution with transitive support
- ✅ 10/10 unit tests passing

Planned:
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
| `sp add <dep>` fails | Dependency not in local registry yet | Only path dependencies are currently supported |
