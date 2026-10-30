use std::net::IpAddr;

use crate::ports::{ExitIpProbe, TailnetProbe};
use crate::{ExitInsights, Result, TailnetHealth};

/// [`ExitIpProbe`] with a fixed answer; `None` simulates a failed lookup.
#[derive(Clone, Copy, Debug)]
pub struct FakeExitIp(pub Option<ExitInsights>);

impl FakeExitIp {
    /// Protected exit at `ip` with no country.
    pub fn at(ip: IpAddr) -> Self {
        Self(Some(ExitInsights {
            ip,
            country: None,
            protected: Some(true),
        }))
    }
}

impl ExitIpProbe for FakeExitIp {
    fn exit(&self) -> Result<ExitInsights> {
        self.0
            .ok_or_else(|| crate::Error::Api("fake: lookup failed".into()))
    }
}

/// [`TailnetProbe`] with a fixed answer.
#[derive(Clone, Debug)]
pub struct FakeTailnet(pub TailnetHealth);

impl TailnetProbe for FakeTailnet {
    fn tailnet(&self) -> Result<TailnetHealth> {
        Ok(self.0.clone())
    }
}
