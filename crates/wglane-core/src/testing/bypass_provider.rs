use crate::ports::BypassProvider;
use crate::{BypassSet, Result};

/// [`BypassProvider`] with a fixed set.
#[derive(Clone, Debug, Default)]
pub struct FakeBypassProvider(pub BypassSet);

impl BypassProvider for FakeBypassProvider {
    fn collect(&self) -> Result<BypassSet> {
        Ok(self.0.clone())
    }
}
