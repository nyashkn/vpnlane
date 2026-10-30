# Releasing vpnlane

Nothing publishes until a human merges the Release PR. release-please only opens and updates that PR; it never merges it.

## Flow

Which prefix to use, and why the PR title must carry it: see [STYLE.md](STYLE.md#commits-and-pr-titles).

1. Commit to `main` (through PRs) with Conventional Commits: `feat:` appears under What's New, `fix:`/`perf:` under Fixes; `docs`, `test`, `chore`, `ci`, `refactor`, `build` are hidden. While below 1.0, `feat` bumps the minor version and a breaking change bumps the minor too.
2. Each push to `main` opens or updates one Release PR (`.github/workflows/release-please.yml`). It bumps the workspace version in `Cargo.toml`, the `vpnlane-*` and `xtask` entries in `Cargo.lock`, `.release-please-manifest.json`, and writes the `CHANGELOG.md` section.
3. A Claude step (`release-notes.yml`, guide in `.github/release-notes-guide.md`) rewrites that section into What's New / Fixes / Smaller changes. It skips cleanly if its token is absent.
4. Review the Release PR, especially the changelog. CI (`ci`) must pass; it includes `just check-versions`, which fails if the manifest, workspace version and `Cargo.lock` entries disagree.
5. Merge it. That creates tag `vX.Y.Z`, which starts `.github/workflows/release.yml` (cargo-dist): two macOS archives (Apple silicon and Intel, each with `vpnlane` and `vpnlane-helper`) and the GitHub Release. Nothing else is published.
6. Check: the release page has both `vpnlane-cli-*-apple-darwin.tar.xz` archives and their `.sha256` files.

Homebrew is not wired up yet. To add it later: set `installers = ["homebrew"]`, `tap = "nyashkn/homebrew-tap"` and `publish-jobs = ["homebrew"]` in `dist-workspace.toml`, run `dist generate`, create the public tap repo (with a `Formula/` dir) and add a `HOMEBREW_TAP_TOKEN` secret with write access to it.

Pull requests run `dist plan` only; they never publish.

## One-time owner setup

1. `repository`/`homepage` in the root `Cargo.toml` point at `https://github.com/nyashkn/vpnlane`; change them only if the repo moves.
2. Repository secrets (create them yourself; never commit them):
   - `RELEASE_PLEASE_TOKEN`: fine-grained personal access token for this repo with contents and pull-requests write, plus actions write (to dispatch the notes workflow). Required so the tag it creates starts `release.yml` and the Release PR gets CI; events from the default `GITHUB_TOKEN` start nothing.
   - `CLAUDE_CODE_OAUTH_TOKEN`: lets the notes rewrite run. Optional; without it the generated changelog is used as is.
3. Settings: Actions may create pull requests is not needed when the PAT is used. Keep `main` protected (PR required, check `ci`).
4. Local tooling for previews: `dist` at the version in `cargo-dist-version`.

## Archive contents

`vpnlane` and `vpnlane-helper` are both bins of `vpnlane-cli`, so one archive (and one formula, `vpnlane-cli.rb`) per target carries both. `vpnlane-macos` is a library and is excluded from dist.

## Local checks

- `just ci` is the gate, including `just check-versions`.
- `dist plan` validates config and lists artifacts. `dist generate` regenerates `release.yml` after editing `dist-workspace.toml`; never hand-edit the workflow.

## Superseded: cargo-release

`release.toml` and `cargo xtask release` predate this flow. `cargo release` bumps and tags locally, which would fight release-please's manifest; do not use it for real releases. `cargo xtask release` (a local release build) is harmless.

## Rename history (wglane -> vpnlane)

The GitHub repo was renamed to `nyashkn/vpnlane` and all URLs now point at it; GitHub redirects the old ones. From 0.5.0 archives are `vpnlane-cli-<target>.tar.xz` (older releases keep `wglane-cli-*`). Release-please's PR branch is `release-please--branches--main--components--vpnlane`. Workflow guards use `github.repository_owner == 'nyashkn'`, so a repo rename cannot silently skip them. Never create a new repo named `wglane`: that would break the redirects.
