//! Browser delivery state only; gameplay results remain owned by App/Simulation.
//! A saved report is an immutable snapshot taken before scenario teardown.

use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct Lifecycle {
    started: bool,
    external: bool,
    match_running: bool,
    pending_match: Option<Value>,
    exit_reason: Option<&'static str>,
    error: Option<String>,
    terminal: bool,
    external_finished: bool,
    /// A panic: memory and any borrow it interrupted can no longer be trusted.
    fatal: bool,
}

impl Lifecycle {
    pub(crate) fn started(&self) -> bool {
        self.started
    }

    /// Launch files and user data may change: nothing runs and nothing panicked.
    pub(crate) fn idle(&self) -> bool {
        !self.fatal && (!self.started || self.terminal)
    }

    /// A `start_spawn` launch that exited cleanly left App and winit torn down.
    /// The imported files outlive it, so the next launch only builds a new App.
    fn may_restart(&self, external: bool) -> bool {
        external && self.external && self.terminal && !self.fatal
    }

    pub(crate) fn may_start(&self, external: bool) -> bool {
        self.may_restart(external) || (!self.started && !self.terminal)
    }

    pub(crate) fn start(&mut self, external: bool) -> Result<(), String> {
        if !self.may_start(external) {
            return Err(crate::i18n::tr("VERA:GameAlreadyStarted"));
        }
        *self = Self::default();
        self.started = true;
        self.external = external;
        Ok(())
    }

    pub(crate) fn spawn_running(&self) -> bool {
        self.started && self.external && self.exit_reason.is_none() && !self.terminal
    }

    pub(crate) fn match_started(&mut self) {
        if !self.terminal && self.exit_reason.is_none() {
            self.match_running = true;
        }
    }

    pub(crate) fn match_running(&self) -> bool {
        self.match_running && !self.terminal
    }

    pub(crate) fn match_ended(&mut self, report: Value) -> Option<Value> {
        if !self.match_running() {
            return None;
        }
        self.match_running = false;
        if self.external {
            // Block callback reentry before notifying the host. The existing
            // network owners finish their leave/finish before delivery.
            self.exit_reason.get_or_insert("match-ended");
            self.pending_match = Some(report);
            None
        } else {
            Some(json!({ "type": "match-ended", "match": report }))
        }
    }

    pub(crate) fn request_exit(&mut self, error: Option<&str>) {
        if let Some(error) = error {
            self.error.get_or_insert_with(|| error.to_owned());
            self.exit_reason = Some("error");
        } else {
            self.exit_reason.get_or_insert("quit");
        }
    }

    pub(crate) fn external_finished(&self) -> bool {
        self.external_finished
    }

    pub(crate) fn finish_external(&mut self, report: Value) -> Option<Value> {
        self.external_finished = true;
        self.request_exit(report["reason"].as_str());
        let mut saved = self.pending_match.take()?;
        // Retain the scenario snapshot, but use the launch owner's final
        // outcome/error and duration after its network handoff.
        saved.as_object_mut()?.extend(report.as_object()?.clone());
        Some(json!({ "type": "match-ended", "match": saved }))
    }

    pub(crate) fn exited(&mut self) -> Option<Value> {
        if self.terminal {
            return None;
        }
        self.terminal = true;
        Some(json!({
            "type": "engine-exited",
            "reason": self.exit_reason.unwrap_or("quit"),
            "error": self.error,
        }))
    }

    pub(crate) fn fatal(&mut self, error: &str) -> Option<Value> {
        if self.terminal {
            return None;
        }
        self.request_exit(Some(error));
        self.terminal = true;
        self.fatal = true;
        Some(json!({ "type": "engine-error", "error": error }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_delivers_frozen_match_before_one_terminal_event_and_rejects_reentry() {
        let mut lifecycle = Lifecycle::default();
        lifecycle.start(true).unwrap();
        lifecycle.match_started();
        assert!(
            lifecycle
                .match_ended(json!({"result": "won", "scores": [{"kills": 7}]}))
                .is_none()
        );
        assert!(!lifecycle.spawn_running());
        assert!(lifecycle.start(true).is_err());
        let matched = lifecycle
            .finish_external(json!({"result": "won", "frames": 42, "reason": null}))
            .unwrap();
        assert_eq!(matched["match"]["scores"][0]["kills"], 7);
        assert_eq!(matched["match"]["frames"], 42);
        assert!(
            lifecycle
                .finish_external(json!({"result": "won"}))
                .is_none()
        );
        assert_eq!(lifecycle.exited().unwrap()["reason"], "match-ended");
        assert!(lifecycle.exited().is_none());
        assert!(lifecycle.start(false).is_err());
    }

    #[test]
    fn a_clean_spawn_exit_admits_another_launch_with_fresh_state() {
        let mut lifecycle = Lifecycle::default();
        lifecycle.start(true).unwrap();
        lifecycle.match_started();
        assert!(!lifecycle.idle());
        lifecycle.match_ended(json!({"result": "won"}));
        lifecycle.finish_external(json!({"result": "won", "reason": null}));
        lifecycle.exited().unwrap();
        assert!(lifecycle.idle());
        lifecycle.start(true).unwrap();
        assert!(!lifecycle.idle() && lifecycle.spawn_running() && !lifecycle.external_finished());
        assert!(lifecycle.exited().is_some());
    }

    #[test]
    fn a_panic_and_a_menu_launch_never_restart() {
        let mut panic = Lifecycle::default();
        panic.start(true).unwrap();
        panic.fatal("panic");
        assert!(panic.start(true).is_err() && !panic.idle());
        let mut menu = Lifecycle::default();
        menu.start(false).unwrap();
        menu.exited().unwrap();
        assert!(menu.start(false).is_err() && menu.start(true).is_err());
    }

    #[test]
    fn ordinary_matches_notify_once_per_install_without_exiting_the_engine() {
        let mut lifecycle = Lifecycle::default();
        lifecycle.start(false).unwrap();
        assert!(lifecycle.match_ended(json!({"result": "quit"})).is_none());
        lifecycle.match_started();
        assert_eq!(
            lifecycle.match_ended(json!({"result": "lost"})).unwrap()["match"]["result"],
            "lost"
        );
        assert!(lifecycle.match_ended(json!({"result": "quit"})).is_none());
        lifecycle.match_started();
        assert!(lifecycle.match_ended(json!({"result": "quit"})).is_some());
        assert_eq!(lifecycle.exited().unwrap()["reason"], "quit");
    }

    #[test]
    fn failed_start_and_panic_do_not_invent_a_finished_match_or_clean_shutdown() {
        let mut lifecycle = Lifecycle::default();
        lifecycle.start(true).unwrap();
        assert!(
            lifecycle
                .finish_external(json!({"result": "failed", "reason": "missing rulesmd.ini"}))
                .is_none()
        );
        let event = lifecycle.exited().unwrap();
        assert_eq!(event["reason"], "error");
        assert_eq!(event["error"], "missing rulesmd.ini");
        let mut panic = Lifecycle::default();
        assert_eq!(panic.fatal("panic").unwrap()["type"], "engine-error");
        assert!(panic.exited().is_none());
    }
}
