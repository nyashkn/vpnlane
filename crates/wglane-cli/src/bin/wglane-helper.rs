//! Root-side helper. Invoked as `sudo -n wglane-helper <verb> [args]`.
//!
//! Security contract: fixed verbs only (`up <CC>` with the key material on
//! stdin, `down`, `bypass <gw> <cidr>...`), every argument validated, configs
//! re-rendered into the root-owned state dir (never read from the caller),
//! no paths from the caller, no shell, caller environment discarded. All of
//! it lives in `wglane_macos::helper_main` so it can be unit tested.

use std::process::ExitCode;

fn main() -> ExitCode {
    wglane_macos::helper_main()
}
