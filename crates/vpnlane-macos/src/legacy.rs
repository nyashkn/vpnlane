//! The pre-rename root layout (`wglane`): detect a live old tunnel and remove
//! the old artifacts after the new install is verified.
//!
//! Root-adjacent, so conservative: nothing is deleted unless it is positively
//! identified by `lstat` (never following a symlink) as a root-owned,
//! non-g/o-writable regular file or directory of the expected name. No
//! recursive delete, no symlinks followed, no command is run from the old install.

use std::fmt;
use std::fs;
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use vpnlane_core::brand::{
    LEGACY_BIN, LEGACY_HELPER_PATH, LEGACY_STATE_DIR, LEGACY_SUDOERS_PATH, LEGACY_TOOL_DIR,
    LEGACY_WG_NAME_FILE,
};
use vpnlane_core::{Error, Result};

use crate::helper::{LOCK_FILE, TOOLS};
use crate::privilege::{lstat, render_sudoers_rule, require_trusted_chain};
use crate::tunnel::{STATE_FILE, WG_RUN_DIR};

/// What happened to one old artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// New artifact written.
    Installed,
    /// Old artifact deleted.
    Removed,
    /// Left alone on purpose (absent, modified by the user, or not empty).
    Skipped,
    /// Not what we expected (symlink, wrong owner, ...): left alone, needs a human.
    Refused,
    /// An OS error.
    Failed,
}

/// One line of the `install-helper` report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What it happened to.
    pub outcome: Outcome,
    /// The path or the subject.
    pub what: String,
    /// Why, when it is not obvious.
    pub note: String,
}

impl Step {
    pub(crate) fn new(outcome: Outcome, what: impl fmt::Display, note: impl Into<String>) -> Self {
        Self {
            outcome,
            what: what.to_string(),
            note: note.into(),
        }
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self.outcome {
            Outcome::Installed => "installed",
            Outcome::Removed => "removed  ",
            Outcome::Skipped => "skipped  ",
            Outcome::Refused => "REFUSED  ",
            Outcome::Failed => "FAILED   ",
        };
        write!(f, "{tag} {}", self.what)?;
        if self.note.is_empty() {
            Ok(())
        } else {
            write!(f, " ({})", self.note)
        }
    }
}

/// Where the old release put things. Production values from [`Legacy::system`].
#[derive(Debug, Clone)]
pub(crate) struct Legacy {
    pub(crate) helper: PathBuf,
    pub(crate) sudoers: PathBuf,
    pub(crate) tools: PathBuf,
    pub(crate) state: PathBuf,
    /// Where the old wireguard-go wrote its name file.
    pub(crate) run_dir: PathBuf,
}

enum Probe {
    Absent,
    Ok,
    Bad(String),
}

impl Legacy {
    pub(crate) fn system() -> Self {
        Self {
            helper: LEGACY_HELPER_PATH.into(),
            sudoers: LEGACY_SUDOERS_PATH.into(),
            tools: LEGACY_TOOL_DIR.into(),
            state: LEGACY_STATE_DIR.into(),
            run_dir: WG_RUN_DIR.into(),
        }
    }

    /// Fails if an old tunnel may still be up. Fails closed on anything it
    /// cannot read. Runs before any change.
    pub(crate) fn refuse_if_tunnel_up(&self) -> Result<()> {
        let tunnels = self.state.join(STATE_FILE);
        let name = self.run_dir.join(LEGACY_WG_NAME_FILE);
        let why = match (fs::symlink_metadata(&tunnels), fs::symlink_metadata(&name)) {
            (Err(e), _) if e.kind() != ErrorKind::NotFound => {
                format!("cannot read {}: {e}", tunnels.display())
            }
            (_, Err(e)) if e.kind() != ErrorKind::NotFound => {
                format!("cannot read {}: {e}", name.display())
            }
            (Ok(m), _) if !(m.file_type().is_file() && m.len() == 0) => {
                format!("{} lists a tunnel", tunnels.display())
            }
            (_, Ok(_)) => format!("{} exists", name.display()),
            _ => return Ok(()),
        };
        Err(Error::Api(format!(
            "an old {LEGACY_BIN} tunnel may still be up ({why}). Run the OLD binary's `{LEGACY_BIN} down` first, then re-run `{}`. Nothing was changed.",
            vpnlane_core::brand::INSTALL_HINT
        )))
    }

    /// `lstat` without following; must be a root-owned (`owner`) non-g/o-writable regular file or dir.
    fn probe(path: &Path, dir: bool, owner: u32) -> Probe {
        let m = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == ErrorKind::NotFound => return Probe::Absent,
            Err(e) => return Probe::Bad(e.to_string()),
        };
        let t = m.file_type();
        if !(if dir { t.is_dir() } else { t.is_file() }) {
            return Probe::Bad(format!(
                "not a regular {} (symlink or other)",
                if dir { "directory" } else { "file" }
            ));
        }
        if m.uid() != owner || m.mode() & 0o022 != 0 {
            return Probe::Bad(format!(
                "owner uid {} mode {:o}: need root-owned, not group/world-writable",
                m.uid(),
                m.mode() & 0o7777
            ));
        }
        Probe::Ok
    }

    /// Canonical parent of `path` is a trusted chain, so nobody else can swap what we delete.
    fn parent_trusted(path: &Path, owner: u32, top: &Path) -> std::result::Result<(), String> {
        let parent = path.parent().ok_or("no parent directory")?;
        let canon = fs::canonicalize(parent).map_err(|e| e.to_string())?;
        require_trusted_chain(&canon, top, owner, lstat).map_err(|e| e.to_string())
    }

    /// Removes the old artifacts in the safe order: rule, helper, tools, state.
    /// Call only after the new install verified.
    pub(crate) fn remove(&self, user: Option<&str>, owner: u32, top: &Path) -> Vec<Step> {
        let mut steps = Vec::new();
        let rule_gone = self.remove_rule(user, owner, top, &mut steps);
        if rule_gone {
            self.remove_helper(owner, top, &mut steps);
            self.remove_tools(owner, top, &mut steps);
        } else {
            steps.push(Step::new(
                Outcome::Skipped,
                format!("{} and {}", self.helper.display(), self.tools.display()),
                "the old sudoers rule is still in place and refers to them; remove the rule, then re-run",
            ));
        }
        self.remove_state(owner, top, &mut steps);
        steps
    }

    /// True when no old rule remains (absent or removed).
    fn remove_rule(
        &self,
        user: Option<&str>,
        owner: u32,
        top: &Path,
        steps: &mut Vec<Step>,
    ) -> bool {
        let p = &self.sudoers;
        let step = |o, n: String| Step::new(o, p.display(), n);
        match Self::probe(p, false, owner) {
            Probe::Absent => {
                steps.push(step(Outcome::Skipped, "not present".into()));
                return true;
            }
            Probe::Bad(w) => {
                steps.push(step(Outcome::Refused, w));
                return false;
            }
            Probe::Ok => {}
        }
        if let Err(w) = Self::parent_trusted(p, owner, top) {
            steps.push(step(Outcome::Refused, w));
            return false;
        }
        // Exactly what the old release generated for this user, or we do not own it.
        let Some(Ok(expected)) = user.map(|u| render_sudoers_rule(u, &self.helper)) else {
            steps.push(step(
                Outcome::Skipped,
                "WARNING: cannot reconstruct the old rule without SUDO_USER; delete it yourself"
                    .into(),
            ));
            return false;
        };
        let read = fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
            .open(p)
            .and_then(|f| {
                let mut buf = Vec::new();
                f.take(4096).read_to_end(&mut buf).map(|_| buf)
            });
        match read {
            Ok(b) if b == expected.as_bytes() => match fs::remove_file(p) {
                Ok(()) => {
                    steps.push(step(
                        Outcome::Removed,
                        "matched the old generated rule exactly".into(),
                    ));
                    true
                }
                Err(e) => {
                    steps.push(step(Outcome::Failed, e.to_string()));
                    false
                }
            },
            Ok(_) => {
                steps.push(step(
                    Outcome::Skipped,
                    "WARNING: differs from the rule the old version generates for you (edited, or another user's); delete it yourself if you do not need it".into(),
                ));
                false
            }
            Err(e) => {
                steps.push(step(Outcome::Failed, format!("reading: {e}")));
                false
            }
        }
    }

    fn remove_helper(&self, owner: u32, top: &Path, steps: &mut Vec<Step>) {
        let p = &self.helper;
        let (o, n) = match Self::probe(p, false, owner) {
            Probe::Absent => (Outcome::Skipped, "not present".to_owned()),
            Probe::Bad(w) => (Outcome::Refused, w),
            Probe::Ok => match Self::parent_trusted(p, owner, top) {
                Err(w) => (Outcome::Refused, w),
                Ok(()) => match fs::remove_file(p) {
                    Ok(()) => (Outcome::Removed, String::new()),
                    Err(e) => (Outcome::Failed, e.to_string()),
                },
            },
        };
        steps.push(Step::new(o, p.display(), n));
    }

    /// Only the known tools, then the (then empty) `bin` dir and its parent; no recursion.
    fn remove_tools(&self, owner: u32, top: &Path, steps: &mut Vec<Step>) {
        let bin = &self.tools;
        let fail = |o, n: String| Step::new(o, bin.display(), n);
        match Self::probe(bin, true, owner) {
            Probe::Absent => return steps.push(fail(Outcome::Skipped, "not present".into())),
            Probe::Bad(w) => return steps.push(fail(Outcome::Refused, w)),
            Probe::Ok => {}
        }
        if let Err(w) = Self::parent_trusted(bin, owner, top) {
            return steps.push(fail(Outcome::Refused, w));
        }
        let files: Vec<PathBuf> = TOOLS.iter().map(|t| bin.join(t)).collect();
        // Identify everything before deleting anything.
        for f in &files {
            if let Probe::Bad(w) = Self::probe(f, false, owner) {
                return steps.push(Step::new(Outcome::Refused, f.display(), w));
            }
        }
        for f in &files {
            if let Err(e) = fs::remove_file(f)
                && e.kind() != ErrorKind::NotFound
            {
                return steps.push(Step::new(Outcome::Failed, f.display(), e.to_string()));
            }
        }
        match fs::remove_dir(bin) {
            Ok(()) => {
                steps.push(fail(Outcome::Removed, String::new()));
                // The parent only if now empty (rmdir refuses otherwise) and ours.
                if let Some(parent) = bin.parent()
                    && matches!(Self::probe(parent, true, owner), Probe::Ok)
                    && fs::remove_dir(parent).is_ok()
                {
                    steps.push(Step::new(Outcome::Removed, parent.display(), ""));
                }
            }
            Err(e) => steps.push(fail(
                Outcome::Skipped,
                format!("contains other files, left in place ({e})"),
            )),
        }
    }

    /// Only `lock` and an empty `tunnels`, then the directory if nothing else is in it.
    fn remove_state(&self, owner: u32, top: &Path, steps: &mut Vec<Step>) {
        let dir = &self.state;
        let s = |o, n: String| Step::new(o, dir.display(), n);
        match Self::probe(dir, true, owner) {
            Probe::Absent => return steps.push(s(Outcome::Skipped, "not present".into())),
            Probe::Bad(w) => return steps.push(s(Outcome::Refused, w)),
            Probe::Ok => {}
        }
        if let Err(w) = Self::parent_trusted(dir, owner, top) {
            return steps.push(s(Outcome::Refused, w));
        }
        let names: io::Result<Vec<_>> =
            fs::read_dir(dir).and_then(|r| r.map(|e| e.map(|e| e.file_name())).collect());
        let names = match names {
            Ok(n) => n,
            Err(e) => return steps.push(s(Outcome::Failed, e.to_string())),
        };
        if let Some(other) = names.iter().find(|n| *n != LOCK_FILE && *n != STATE_FILE) {
            return steps.push(s(
                Outcome::Skipped,
                format!("contains {}, left in place", other.to_string_lossy()),
            ));
        }
        for n in &names {
            let f = dir.join(n);
            let empty_ok = fs::symlink_metadata(&f).is_ok_and(|m| m.len() == 0);
            if !matches!(Self::probe(&f, false, owner), Probe::Ok) || !empty_ok {
                return steps.push(s(
                    Outcome::Skipped,
                    format!(
                        "{} is not an empty root-owned file, left in place",
                        n.to_string_lossy()
                    ),
                ));
            }
        }
        // An old helper still running holds this lock.
        if let Ok(f) = fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits().cast_signed())
            .open(dir.join(LOCK_FILE))
            && rustix::fs::flock(&f, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err()
        {
            return steps.push(s(
                Outcome::Skipped,
                "an old helper is running; re-run later".into(),
            ));
        }
        for n in &names {
            if let Err(e) = fs::remove_file(dir.join(n)) {
                return steps.push(s(Outcome::Failed, e.to_string()));
            }
        }
        steps.push(match fs::remove_dir(dir) {
            Ok(()) => s(Outcome::Removed, String::new()),
            Err(e) => s(Outcome::Failed, e.to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The byte-for-byte rule `wglane` 0.4 wrote (`{user} ALL=(root) NOPASSWD: {helper}\n`).
    #[test]
    fn reconstructs_the_old_rule_exactly() {
        let l = Legacy::system();
        assert_eq!(l.helper, Path::new("/usr/local/libexec/wglane-helper"));
        assert_eq!(
            render_sudoers_rule("kn", &l.helper).unwrap(),
            "kn ALL=(root) NOPASSWD: /usr/local/libexec/wglane-helper\n"
        );
    }
}
