use std::cell::RefCell;
use std::path::Path;

use crate::Result;
use crate::ports::Tunnel;

/// [`Tunnel`] that tracks which tunnels are "up".
#[derive(Debug, Default)]
pub struct FakeTunnel {
    /// Names currently up, in order brought up.
    pub up: RefCell<Vec<String>>,
}

fn name(conf: &Path) -> String {
    conf.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Tunnel for FakeTunnel {
    fn up(&self, conf: &Path) -> Result<()> {
        self.up.borrow_mut().push(name(conf));
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
