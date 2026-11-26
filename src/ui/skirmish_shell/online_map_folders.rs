//! The website's category memberships, shared with the open chooser. A local
//! copy becomes a library member only after the catalog scan matches its bytes.

use std::collections::HashMap;
use std::sync::Arc;

use crate::map::skirmish_scenarios::{SkirmishScenarioRecord, SkirmishScenarioSource};

#[derive(Debug, Default, Clone)]
pub(crate) struct OnlineMapFolders {
    memberships: Arc<HashMap<String, Vec<String>>>,
    /// Actual scenario filename to the library SHA matched on disk.
    installed: Arc<HashMap<String, String>>,
}

impl OnlineMapFolders {
    pub(crate) fn has_categories(&self) -> bool {
        !self.memberships.is_empty()
    }

    pub(crate) fn includes(&self, record: &SkirmishScenarioRecord, category: Option<&str>) -> bool {
        let sha = match &record.source {
            SkirmishScenarioSource::Online(sha) => sha,
            _ => match self.installed.get(&record.file_name) {
                Some(sha) => sha,
                None => return false,
            },
        };
        category.is_none_or(|id| {
            self.memberships
                .get(sha)
                .is_some_and(|ids| ids.iter().any(|known| known == id))
        })
    }

    pub(crate) fn set_memberships(&mut self, memberships: HashMap<String, Vec<String>>) {
        self.memberships = Arc::new(memberships);
    }

    pub(crate) fn reset_installed(&mut self) {
        self.installed = Arc::default();
    }

    pub(crate) fn record_installed(&mut self, file_name: &str, digest: &str) {
        Arc::make_mut(&mut self.installed).insert(file_name.to_owned(), digest.to_owned());
    }

    #[cfg(test)]
    pub(crate) fn from_memberships(memberships: HashMap<String, Vec<String>>) -> Self {
        Self {
            memberships: Arc::new(memberships),
            ..Self::default()
        }
    }
}
