//! The browser build has no module runtime yet (RFC 0001 open question 1).

use crate::{Export, Fault, Limits, LoadError, SimHost};

/// Uninhabited: no module ever loads in the browser build.
pub enum SimModule {}

impl SimModule {
    pub fn load(_bytes: &[u8], _limits: Limits) -> Result<Self, LoadError> {
        Err(LoadError::Unsupported(
            "the browser build cannot run Wasm extension modules yet".into(),
        ))
    }

    pub fn has_export(&self, _export: Export) -> bool {
        match *self {}
    }

    pub fn call(
        &mut self,
        _export: Export,
        _budget: u64,
        _host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        match *self {}
    }

    pub fn memory(&self) -> &[u8] {
        match *self {}
    }
}
