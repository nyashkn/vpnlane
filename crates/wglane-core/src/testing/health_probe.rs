use std::net::IpAddr;

use crate::ports::{ExitIpProbe, TailnetProbe};
use crate::{Result, TailnetHealth};

/// [`ExitIpProbe`] with a fixed IP.
#[derive(Clone, Copy, Debug)]
pub struct FakeExitIp(pub IpAddr);

impl ExitIpProbe for FakeExitIp {
    fn exit_ip(&self) -> Result<IpAddr> {
        Ok(self.0)
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
