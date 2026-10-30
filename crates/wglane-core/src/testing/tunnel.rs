use std::cell::RefCell;
use std::path::Path;

use crate::Result;
use crate::ports::Tunnel;

/// [`Tunnel`] that tracks which tunnels are "up".
#[derive(Default)]
pub struct FakeTunnel {
    /// Names currently up, in order brought up.
    pub up: RefCell<Vec<String>>,
    /// Run after each successful `up`, e.g. to move the physical gateway.
    pub on_up: RefCell<Vec<Box<dyn Fn()>>>,
}

impl std::fmt::Debug for FakeTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeTunnel")
            .field("up", &self.up)
            .finish_non_exhaustive()
    }
}

fn name(conf: &Path) -> String {
    conf.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Tunnel for FakeTunnel {
    fn up(&self, conf: &Path) -> Result<()> {
        self.up.borrow_mut().push(name(conf));
        self.on_up.borrow().iter().for_each(|f| f());
        Ok(())
    }

    fn down(&self, conf: &Path) -> Result<()> {
        let n = name(conf);
        self.up.borrow_mut().retain(|u| *u != n);
        Ok(())
    }

    fn active(&self) -> Result<Vec<String>> {
        Ok(self.up.borrow().clone())
    }
}
