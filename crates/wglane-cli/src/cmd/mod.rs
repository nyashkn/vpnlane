//! One module per subcommand. Each defines `Args` (with its own help text)
//! and `run(&Context, &Args)`.

pub(crate) mod completions;
pub(crate) mod doctor;
pub(crate) mod down;
pub(crate) mod gen_conf;
pub(crate) mod install_helper;
pub(crate) mod servers;
pub(crate) mod status;
pub(crate) mod tui;
pub(crate) mod up;
pub(crate) mod watch;
