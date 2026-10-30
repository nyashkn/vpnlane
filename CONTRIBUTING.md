# Contributing

## Setup

```sh
brew install just
cargo install cargo-deny
just ci            # the single gate: fmt, clippy -D warnings, test, deny, doc, no-tui build
```

`rust-toolchain.toml` pins the compiler; rustup installs it on first use.
Tests need no network, no root and no WireGuard install.

Other recipes: `just fmt`, `just test`, `just run status --json`,
`just assets` (completions and man pages into `target/assets/`).

## Where code goes

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): crate graph, port/adapter
table, file ownership map, and "where does X go". Style rules are in
[docs/STYLE.md](docs/STYLE.md).

## Snapshots

`insta` snapshots live in `snapshots/` next to the code. After an intended
output change:

```sh
INSTA_UPDATE=always just test
git diff -- '*.snap'      # review every changed line
```

## Add a subcommand

1. Create `crates/wglane-cli/src/cmd/<name>.rs` with `Args` (`#[derive(clap::Args)]`
   and `#[command(about, long_about, after_help = "Examples: ...")]`) and
   `pub(crate) fn run(ctx: &Context, args: &Args) -> Result<()>`. Copy
   `cmd/completions.rs`.
2. Add `pub(crate) mod <name>;` to `cmd/mod.rs`, a variant to `Command` in
   `cli.rs`, and an arm to the match in `lib.rs`.
3. `INSTA_UPDATE=always just test` to refresh the top-level help snapshot.
   `every_subcommand_has_examples` fails if `after_help` is missing.

## Add an adapter

1. If no port fits, add one in `wglane-core/src/ports/<port>.rs` with doc
   comments, plus a fake in `src/testing/<port>.rs`.
2. Implement it in the adapter crate, one file per adapter. Run external
   programs through `CommandRunner`; HTTP through `NordApi`'s agent.
3. Test with `FakeRunner` / fixtures. Copy the style of
   `crates/wglane-nord/src/token.rs`.
4. Wire it in the subcommand that needs it.

## Commits

Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`,
`chore:`). One concern per commit. `just ci` green before pushing.
