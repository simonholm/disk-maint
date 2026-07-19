# disk-maint

`disk-maint` is a maintenance-oriented companion to `disk-agent`.

`disk-agent` observes and explains disk usage without modifying the system.
`disk-maint` may perform cleanup, but only after showing the planned action and
asking for explicit confirmation.

## Philosophy

- Default behavior is read-only.
- Cleanup commands summarize what will be changed before doing anything.
- Destructive operations are never automatic.
- Prefer official ecosystem commands where practical.
- Every cleanup action explains why it is considered safe and what the
  trade-offs are.

## Install

```sh
cargo install --path . --locked
```

## Commands

```sh
disk-maint scan
disk-maint rust
disk-maint git status
disk-maint clean target
disk-maint clean target --dry-run
disk-maint clean target --yes
disk-maint clean shared
disk-maint clean shared --yes
```

Use `--root` to scan a different repository root:

```sh
disk-maint --root ~/labs/repos rust
disk-maint --root ~/labs/repos git status
disk-maint --root /tmp/repos clean target
disk-maint --root /tmp/repos clean shared
```

The default root is `~/labs/repos`.

## `disk-maint scan`

Prints a high-level Rust maintenance report without modifying anything:

- Cargo build artifacts under project `target/` directories
- Cargo registry cache
- Cargo git cache
- Installed Rust toolchains
- Rust project count

Registry, git cache, and toolchain entries are informational only in this
initial version.

## `disk-maint rust`

Recursively scans the configured root for Rust projects by locating
`Cargo.toml`. Cargo workspaces are reported as one logical project by default.

For each project, it reports:

- project name
- `target/` size
- source size from typical Rust source paths (`src/`, `tests/`, `benches/`,
  `examples/`, `build.rs`, `Cargo.toml`, and `Cargo.lock`)

Example:

```text
disk-agent
  target/      912M
  source       2.0M

codex-session-tools
  target/      241M
  source       400K

Total reclaimable build artifacts:
1.1G
```

## `disk-maint git status`

Recursively scans the configured root for Git repositories by locating `.git`
directories. Clean repositories are omitted.

Example:

```text
== disk-maint ==
 M README.md
 M src/rust/mod.rs

== mobile-fix-demo ==
?? Cargo.toml
```

If every repository is clean, it prints:

```text
All repositories are clean.
```

## `disk-maint clean target`

Finds Rust `target/` directories beneath the configured root, shows the
projects affected and estimated reclaimable space, then prompts for
confirmation.

Only `target/` directories are removed.

The safety rationale is that `target/` contains Cargo build artifacts that Cargo
can rebuild. The trade-off is that the next build may take longer, and any files
manually placed under `target/` will be lost.

Confirmation requires typing `yes` exactly.

Use `--dry-run` to print the cleanup plan without prompting or deleting
anything:

```sh
disk-maint clean target --dry-run
```

Use `--yes` for automation when you want to delete the planned `target/`
directories without an interactive prompt:

```sh
disk-maint clean target --yes
```

## `disk-maint clean shared`

Finds Cargo's shared target directory by asking Cargo for metadata from Rust
projects beneath the configured root, falling back to `CARGO_TARGET_DIR` when no
metadata result identifies a shared target directory.

This is different from `clean target`: project-local `target/` directories are
owned by individual repositories, while the shared Cargo target directory is a
single build cache reused across projects when Cargo is configured that way.

The command shows the absolute shared target path, total size, and safety
rationale before prompting for confirmation. The safety rationale is that the
directory contains Cargo build artifacts that Cargo can rebuild. The trade-off
is broader than `clean target`: all projects using the shared cache will rebuild
afterwards.

Confirmation requires typing `yes` exactly.

Use `--yes` for automation when you want to delete the shared Cargo target
directory without an interactive prompt:

```sh
disk-maint clean shared --yes
```

If no shared Cargo target directory is found, no prompt is shown and nothing is
deleted.

## Current Scope

Implemented:

- Rust project discovery
- Rust build artifact reporting
- Git working tree status reporting
- Confirmed cleanup of Rust `target/` directories
- Confirmed cleanup of Cargo's shared target directory

Not implemented yet:

- Node
- Python
- Docker
- Podman
- LLM runtimes

The module layout leaves room for future ecosystem-specific commands:

```text
src/
  main.rs
  cli.rs
  scan/
  git/
  rust/
  clean/
```

## Validation

```sh
CARGO_TARGET_DIR=/tmp/disk-maint-cargo-target cargo test
```
