//! Root-side helper. Invoked as `sudo -n wglane-helper <verb> [args]`.
//!
//! Security contract: fixed verbs only (`up <cc>`, `down`, `bypass <gw>
//! <cidr>...`), every argument validated (country code, IP, CIDR), configs
//! read only from the root-owned config dir, no paths from the caller, no
//! shell, environment ignored.

use std::process::ExitCode;

fn main() -> ExitCode {
    // TODO(bead): macos-helper-bin: parse argv by hand (no clap: small attack
    // surface), validate with CountryCode/IpAddr/IpNet parsers, dispatch to
    // WgQuick/RouteTable with SystemRunner. Exit 2 on bad usage.
    eprintln!("wglane-helper: not implemented yet");
    ExitCode::from(2)
}
