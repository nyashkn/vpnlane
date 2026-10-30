# Releasing wglane

Nothing publishes until a human merges the Release PR. release-please only opens and updates that PR; it never merges it.

## Flow

1. Commit to `main` (through PRs) with Conventional Commits: `feat:` appears under What's New, `fix:`/`perf:` under Fixes; `docs`, `test`, `chore`, `ci`, `refactor`, `build` are hidden. While below 1.0, `feat` bumps the minor version and a breaking change bumps the minor too.
2. Each push to `main` opens or updates one Release PR (`.github/workflows/release-please.yml`). It bumps the workspace version in `Cargo.toml`, the `wglane-*` and `xtask` entries in `Cargo.lock`, `.release-please-manifest.json`, and writes the `CHANGELOG.md` section.
3. A Claude step (`release-notes.yml`, guide in `.github/release-notes-guide.md`) rewrites that section into What's New / Fixes / Smaller changes. It skips cleanly if its token is absent.
4. Review the Release PR, especially the changelog. CI (`ci`) must pass; it includes `just check-versions`, which fails if the manifest, workspace version and `Cargo.lock` entries disagree.
5. Merge it. That creates tag `vX.Y.Z`, which starts `.github/workflows/release.yml` (cargo-dist): macOS archives, the GitHub Release, and the Homebrew formula push to the tap.
6. Check: `brew install nyashkn/tap/wglane-cli` (installs both binaries).

Pull requests run `dist plan` only; they never publish.

## One-time owner setup

1. `repository`/`homepage` in the root `Cargo.toml` already point at `https://github.com/nyashkn/wglane`; change them only if the repo moves.
2. Create the tap repo (`nyashkn/homebrew-tap`, public, with a `Formula/` dir). If you use a different name, set `tap` in `dist-workspace.toml`, then run `dist generate`.
3. Repository secrets (create them yourself; never commit them):
   - `HOMEBREW_TAP_TOKEN`: write access to the tap repo (used by the dist release job).
   - `RELEASE_PLEASE_TOKEN`: fine-grained personal access token for this repo with contents and pull-requests write, plus actions write (to dispatch the notes workflow). Required so the tag it creates starts `release.yml` and the Release PR gets CI; events from the default `GITHUB_TOKEN` start nothing.
   - `CLAUDE_CODE_OAUTH_TOKEN`: lets the notes rewrite run. Optional; without it the generated changelog is used as is.
4. Settings: Actions may create pull requests is not needed when the PAT is used. Keep `main` protected (PR required, check `ci`).
5. Local tooling for previews: `dist` at the version in `cargo-dist-version`.

## Archive contents

`wglane` and `wglane-helper` are both bins of `wglane-cli`, so one archive (and one formula, `wglane-cli.rb`) per target carries both. `wglane-macos` is a library and is excluded from dist.

## Local checks

- `just ci` is the gate, including `just check-versions`.
- `dist plan` validates config and lists artifacts. `dist generate` regenerates `release.yml` after editing `dist-workspace.toml`; never hand-edit the workflow.

## Superseded: cargo-release

`release.toml` and `cargo xtask release` predate this flow. `cargo release` bumps and tags locally, which would fight release-please's manifest; do not use it for real releases. `cargo xtask release` (a local release build) is harmless.
