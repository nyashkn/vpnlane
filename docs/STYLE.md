# Style

Baseline: `rustfmt` (config in `rustfmt.toml`) and `clippy -D warnings` with
the workspace lint set in `Cargo.toml`. Beyond that, these rules from the
[Google Rust style guide](https://google.github.io/styleguide/rustguide.html),
filtered for a small CLI.

## Adopted

| Rule | Here |
|---|---|
| Name a file after its primary item | `render.rs` holds `render_wg_conf`, `tunnel.rs` holds `WgGo`. |
| No real code in non-leaf modules | `lib.rs` and `mod.rs` hold `mod` + `pub use` only (plus `cli`'s dispatch match). |
| No glob imports | except `use super::*` in `mod tests`. |
| Generics over `dyn` in libraries | ports are taken as `T: Port`. |
| Choose `Option`/`Result`/panic by contract | `Option` for "not there" (`TokenSource` returns `Ok(None)`), `Result` for external failures, panic never in libraries. |
| `expect` over `unwrap`, with a reason | `unwrap` is a clippy warning outside tests. |
| Prefer enums over bool parameters | `HelperVerb`, `TailnetHealth`. |
| `#[non_exhaustive]` on public enums that will grow | `Error`, `TailnetHealth`. Two-state or closed enums excepted. |
| Newtypes for validated values | `CountryCode`, `Token`, `PrivateKey`. |
| Doc comments on public items | `missing_docs` lint. Say what, when it fails, what to do; don't explain Rust. |
| Avoid `Rc`/`Arc` | none so far; fakes use `RefCell`. |
| `unsafe`: don't | `unsafe_code = "forbid"` workspace-wide. A future exception needs its own module, a `// SAFETY:` comment and review. |
| No unstable features | stable toolchain pinned in `rust-toolchain.toml`. |

## Skipped, and why

| Rule | Why |
|---|---|
| Qualify top-level functions with their module (`mem::take`) | Our free functions are already distinct (`render_wg_conf`, `is_stale`); re-exports from crate roots read better. |
| Don't use async / don't spawn std threads (Chrome) | Chrome-specific runtime rules. We are blocking anyway. |
| Per-codebase test frameworks (gtest, `fuchsia::test`) | Plain `#[test]` + `insta`. |
| Avoid abbreviations | Domain terms `cc`, `gw`, `conf` are clearer than their long forms to anyone who knows WireGuard. |
| Flat `rust_library` layout, import mechanism rules | Bazel/GN-specific. |

## Project rules (not from Google)

- Library errors: `wglane_core::Error` via `thiserror`; messages say what to
  do next. `color-eyre` only in binaries.
- Secrets: never on argv, never in `Debug`, zeroized on drop, files mode 0600.
- `#[allow]` only narrowly, with `reason = "..."`.
- Shortcuts with a known limit get `// shortcut: <limit>, <when to upgrade>`.
