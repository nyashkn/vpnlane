# Releasing wglane

Nothing is published until the owner pushes a `v*` tag. `cargo release` is a dry run unless `--execute` is passed, and `release.toml` sets `push = false` and `publish = false`.

## One-time owner setup

1. `repository`/`homepage` in the root `Cargo.toml` already point at `https://github.com/nyashkn/wglane`; change them only if the repo moves.
2. Create the tap repo (`nyashkn/homebrew-tap`, public, with a `Formula/` dir). If you use a different name, set `tap` in `dist-workspace.toml`, then run `dist generate`.
3. Create a token with write access to the tap repo and add it as the `HOMEBREW_TAP_TOKEN` secret on the wglane repo.
4. `cargo install cargo-release --locked` (and `dist`, same version as `cargo-dist-version`).

## Cutting a release

1. Add entries under `## [Unreleased]` in `CHANGELOG.md`.
2. Preview: `cargo release patch` (or `minor`/`major`). Prints the plan; runs `just ci` as the pre-release hook. Needs a clean tree.
3. Bump + tag locally: `cargo release patch --execute`. All crates share one version; this commits the bump and creates tag `vX.Y.Z`. It does not push.
4. Push: `git push && git push origin vX.Y.Z`. The tag triggers `.github/workflows/release.yml` (cargo-dist): builds aarch64 and x86_64 macOS archives, creates the GitHub Release, and pushes the Homebrew formula to the tap.
5. Check: `brew install nyashkn/tap/wglane-cli` (installs both binaries).

Pull requests run `dist plan` only; they never publish.

## Archive contents

`wglane` and `wglane-helper` are both bins of `wglane-cli`, so one archive (and one formula, `wglane-cli.rb`) per target carries both. `wglane-macos` is a library and is excluded from dist.

## Local checks

`dist plan` validates config and lists artifacts. `dist generate` regenerates `release.yml` after editing `dist-workspace.toml`; never hand-edit the workflow.
