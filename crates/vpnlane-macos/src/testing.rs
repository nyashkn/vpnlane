//! [`FakeRunner`]: scripted [`CommandRunner`] for tests.

use std::cell::RefCell;
use std::collections::VecDeque;

use vpnlane_core::Result;

use crate::{CommandOutput, CommandRunner};

/// Returns queued outputs in order and records every call.
///
/// An empty queue answers with a successful, empty output, so tests only
/// script the calls they care about.
#[derive(Debug, Default)]
pub struct FakeRunner {
    replies: RefCell<VecDeque<CommandOutput>>,
    calls: RefCell<Vec<Vec<String>>>,
    stdins: RefCell<Vec<Vec<u8>>>,
}

impl FakeRunner {
    /// No scripted replies.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues the reply for the next call.
    pub fn push(&self, reply: CommandOutput) -> &Self {
        self.replies.borrow_mut().push_back(reply);
        self
    }

    /// Queues a successful reply with `stdout`.
    pub fn push_stdout(&self, stdout: &str) -> &Self {
        self.push(CommandOutput {
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        })
    }

    /// Records a call without consuming a scripted reply (for wrappers that answer it themselves).
    pub fn note(&self, program: &str, args: &[&str]) {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|a| (*a).to_owned()));
        self.calls.borrow_mut().push(call);
    }

    /// Every call so far as `[program, args...]`.
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.borrow().clone()
    }

    /// Every stdin payload passed to `run_with_stdin`, in order.
    pub fn stdins(&self) -> Vec<Vec<u8>> {
        self.stdins.borrow().clone()
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput> {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|a| (*a).to_owned()));
        self.calls.borrow_mut().push(call);
        Ok(self
            .replies
            .borrow_mut()
            .pop_front()
            .unwrap_or(CommandOutput {
                code: Some(0),
                ..CommandOutput::default()
            }))
    }

    fn run_with_stdin(&self, program: &str, args: &[&str], stdin: &[u8]) -> Result<CommandOutput> {
        self.stdins.borrow_mut().push(stdin.to_vec());
        self.run(program, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_in_order_then_defaults_and_records() {
        let f = FakeRunner::new();
        f.push_stdout("one").push_stdout("two");
        assert_eq!(f.run("a", &["1"]).unwrap().stdout, "one");
        assert_eq!(f.run("b", &[]).unwrap().stdout, "two");
        assert!(f.run("c", &[]).unwrap().success());
        assert_eq!(f.calls(), vec![vec!["a", "1"], vec!["b"], vec!["c"]]);
    }
}
