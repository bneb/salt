---
title: "The End of the Lattice Monorepo"
date: "2026-07-11"
author: "The Lattice Team"
tags: ["Architecture", "Ecosystem", "Announcement"]
---

For the past two years, the Lattice monorepo has served us well. It was the incubator where the Salt compiler, the KeuOS microkernel, the Basalt LLM engine, and the Lettuce KV store all grew up together. Having a single workspace made cross-cutting changes trivial: updating the Z3 verification semantics in the compiler and applying them to the NetD TCP stack in the same commit was a massive productivity multiplier.

But as the ecosystem matures, the monorepo is showing its limits.

## Why Disaggregate?

1. **Independent Versioning**: `salt-front` (the compiler) and `sp` (the package manager) are general-purpose tools. They shouldn't share a release cycle with a microkernel.
2. **Contributor Friction**: Developers who just want to write a userspace app in Salt shouldn't have to clone a repository containing custom QEMU runners, C++ MLIR backends, and full Linux kernels.
3. **Ecosystem Maturation**: With the recent addition of robust Git dependency resolution to our package manager (`sp`), the components can finally stand on their own.

## The Fresh Cut

Starting today, the `bneb/lattice` repository is officially archived as a historical reference. We have migrated the components into their own dedicated, disaggregated repositories:

- **[salt](https://github.com/bneb/salt)**: The Salt systems language compiler (`saltc`), the `sp` package manager, and the standard library.
- **[keuos](https://github.com/bneb/keuos)**: The Z3-verified microkernel, userspace drivers, and QEMU tooling.
- **[basalt](https://github.com/bneb/basalt)**: The native LLM inference engine.
- **[lettuce](https://github.com/bneb/lettuce)**: The ultra-fast KV store.
- **[facet](https://github.com/bneb/facet)**: The 2D UI compositor and windowing system.

## What This Means For You

If you were building projects in the monorepo, you'll need to adapt to the new standalone `sp` workflow. 

**1. Install `sp` globally:**
```bash
cargo install --git https://github.com/bneb/salt sp
```

**2. Use Git dependencies in `salt.toml`:**
You no longer rely on `manifest.salt`. Instead, depend on the components you need directly via Git:
```toml
[dependencies]
salt = { git = "https://github.com/bneb/salt", branch = "main" }
keuos = { git = "https://github.com/bneb/keuos", branch = "main" }
```

We are incredibly excited about this next phase of the project. By decoupling the tooling from the OS, we are taking the first major step toward making Salt a language that anyone can use for any project, anywhere.

Happy coding,
The Lattice Team
