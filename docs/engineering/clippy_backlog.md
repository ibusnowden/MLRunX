# Clippy Backlog

This document tracks intentional `#[allow(...)]` usage that still needs cleanup
before we can enforce broad clippy coverage across the full Rust workspace.

## Current Hotspots

- `apps/api/src/main.rs`
  - crate-level allow list (`#![allow(...)]`) covering:
    - `clippy::module_name_repetitions`
    - `clippy::must_use_candidate`
    - `clippy::missing_errors_doc`
    - `clippy::missing_panics_doc`
    - `clippy::redundant_pub_crate`
    - `clippy::future_not_send`
    - `clippy::significant_drop_tightening`
    - `clippy::option_if_let_else`
    - `dead_code`
    - `unused_imports`
  - local function-level allow annotations (`too_many_lines`, `too_many_arguments`)
- `apps/api/src/auth/mod.rs`
  - local `#[allow(clippy::too_many_lines)]`
- `apps/api/src/storage/sqlite.rs`
  - local allow annotations for `too_many_lines`, `too_many_arguments`, conversion casts

## Burn-Down Plan

1. Split oversized files into smaller modules/crates (policy/types/storage helpers).
2. Remove crate-level `#![allow(...)]` from `apps/api/src/main.rs`.
3. Replace broad `too_many_lines` allows with targeted helper extraction.
4. Convert cast-related allows to explicit checked conversions where possible.
5. Keep CI enforcing `cargo clippy -p mlrunx-api -- -D warnings` while backlog is reduced.

## Gate Policy

- New `#[allow(...)]` additions should be treated as debt and documented here.
- PRs should not expand the allow list without a linked rationale.
