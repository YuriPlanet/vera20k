//! The browser build has no module runtime yet (RFC 0001 open question 1).

use crate::{Export, Fault, Limits, LoadError, ModuleState, SimHost};

/// Never constructed in the browser build: [`SimModule::load`] always fails.
pub struct SimModule {
    _private: (),
}

impl SimModule {
    pub fn load(_bytes: &[u8], _limits: Limits) -> Result<Self, LoadError> {
        Err(LoadError::Unsupported(
            "the browser build cannot run Wasm extension modules yet".into(),
        ))
    }

    pub fn restore(_bytes: &[u8], limits: Limits, _state: &ModuleState) -> Result<Self, LoadError> {
        Self::load(_bytes, limits)
    }

    pub fn bytes(&self) -> &[u8] {
        &[]
    }

    pub fn globals(&self) -> &[u64] {
        &[]
    }

    pub fn state(&self) -> ModuleState {
        ModuleState::default()
    }

    pub fn has_export(&self, _export: Export) -> bool {
        false
    }

    pub fn call(
        &mut self,
        _export: Export,
        _budget: u64,
        _host: &mut dyn SimHost,
    ) -> Result<u64, Fault> {
        Ok(0)
    }

    pub fn memory(&self) -> &[u8] {
        &[]
    }
}
