use std::process::Command;

use wglane_core::{Error, Result};

/// Captured result of one program run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOutput {
    /// Exit code; `None` if killed by a signal.
    pub code: Option<i32>,
    /// Captured stdout (lossy UTF-8).
    pub stdout: String,
    /// Captured stderr (lossy UTF-8).
    pub stderr: String,
}

impl CommandOutput {
    /// Exit code 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runs external programs. The seam that keeps adapters testable.
pub trait CommandRunner {
    /// Runs `program` with `args`, capturing output. Non-zero exit is *not*
    /// an error here; only failure to start is.
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput>;

    /// Like [`run`](Self::run), but non-zero exit becomes `Error::Command`.
    fn run_ok(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let out = self.run(program, args)?;
        if out.success() {
            Ok(out)
        } else {
            Err(Error::Command {
                program: program.to_owned(),
                status: out
                    .code
                    .map_or_else(|| "killed by signal".into(), |c| format!("exit {c}")),
                stderr: out.stderr.trim().to_owned(),
            })
        }
    }
}

/// Real [`CommandRunner`] over `std::process::Command`. No shell is involved,
/// so arguments are never re-parsed.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let out = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| Error::io(format!("running `{program}` (is it installed?)"), e))?;
        Ok(CommandOutput {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeRunner;

    #[test]
    fn system_runner_captures_output_and_code() {
        let ok = SystemRunner
            .run("/bin/sh", &["-c", "echo hi; echo err >&2"])
            .unwrap();
        assert!(ok.success());
        assert_eq!(ok.stdout, "hi\n");
        assert_eq!(ok.stderr, "err\n");
        let bad = SystemRunner.run("/bin/sh", &["-c", "exit 3"]).unwrap();
        assert_eq!(bad.code, Some(3));
    }

    #[test]
    fn system_runner_missing_program_is_io_error() {
        let err = SystemRunner
            .run("/nonexistent/wglane-test", &[])
            .unwrap_err();
        assert!(matches!(err, Error::Io { .. }));
    }

    #[test]
    fn run_ok_turns_failure_into_command_error() {
        let fake = FakeRunner::new();
        fake.push(CommandOutput {
            code: Some(1),
            stderr: "boom\n".into(),
            ..Default::default()
        });
        let err = fake.run_ok("wg-quick", &["up", "us"]).unwrap_err();
        assert_eq!(err.to_string(), "`wg-quick` failed (exit 1): boom");
    }
}
