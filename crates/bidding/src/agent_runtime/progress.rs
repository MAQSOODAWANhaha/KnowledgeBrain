//! Role-local progress, evaluated only at committed tool boundaries.
//! Domains supply evidence/result versions; queries and notes are never progress.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub fn default_no_progress_turns() -> usize {
    6
}
pub fn default_focus_turns() -> usize {
    24
}
pub fn default_focus_replans() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProgressLimits {
    pub max_no_progress_turns: usize,
    pub max_focus_turns: usize,
    pub max_focus_replans: usize,
}
impl Default for ProgressLimits {
    fn default() -> Self {
        Self {
            max_no_progress_turns: default_no_progress_turns(),
            max_focus_turns: default_focus_turns(),
            max_focus_replans: default_focus_replans(),
        }
    }
}
impl ProgressLimits {
    pub fn validate(&self) -> bool {
        self.max_no_progress_turns > 0 && self.max_focus_turns > 0 && self.max_focus_replans > 0
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Recovery {
    #[default]
    Running,
    Replan,
    Blocked,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressWatch {
    pub no_progress_turns: usize,
    pub focus_turns: usize,
    pub replans: usize,
    pub recovery: Recovery,
}
impl ProgressWatch {
    /// New evidence can resume execution without finishing the stalled action.
    /// Keep its recovery context until a committed completion resets the watch.
    pub fn needs_replan_context(&self) -> bool {
        self.recovery != Recovery::Blocked
            && (self.recovery == Recovery::Replan || self.replans > 0)
    }

    fn observe(&mut self, changed: bool, completed: bool, limits: &ProgressLimits) {
        if self.recovery == Recovery::Blocked {
            // Give the Agent one bounded handoff window to choose independent
            // work; blocked navigation cannot consume the entire global budget.
            self.no_progress_turns = self.no_progress_turns.saturating_add(1);
            return;
        }
        self.focus_turns = self.focus_turns.saturating_add(1);
        if changed {
            self.no_progress_turns = 0;
        } else {
            self.no_progress_turns = self.no_progress_turns.saturating_add(1);
        }
        if completed {
            *self = Self::default();
            return;
        }
        if self.no_progress_turns >= limits.max_no_progress_turns
            || self.focus_turns >= limits.max_focus_turns
        {
            if self.replans >= limits.max_focus_replans {
                self.recovery = Recovery::Blocked;
                self.no_progress_turns = limits.max_no_progress_turns;
            } else {
                self.replans += 1;
                self.no_progress_turns = 0;
                self.focus_turns = 0;
                self.recovery = Recovery::Replan;
            }
        } else if changed {
            self.recovery = Recovery::Running;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionBlocker {
    pub scope: Vec<String>,
    pub dependencies_sha256: String,
    pub watch: ProgressWatch,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub watch: ProgressWatch,
    /// Novel versions, not the last fingerprint: A/B/A reads are not progress.
    pub seen: BTreeSet<String>,
    pub completions: BTreeSet<String>,
    pub blockers: Vec<ExecutionBlocker>,
}
impl Progress {
    pub fn observe(
        &mut self,
        versions: impl IntoIterator<Item = String>,
        completion: Option<String>,
        limits: &ProgressLimits,
    ) {
        let mut changed = false;
        for version in versions {
            changed |= self.seen.insert(version);
        }
        let completed = completion.is_some_and(|key| self.completions.insert(key));
        self.watch.observe(changed, completed, limits);
    }

    pub fn handoff_exhausted(&self, limits: &ProgressLimits) -> bool {
        self.watch.recovery == Recovery::Blocked
            && self.watch.no_progress_turns >= limits.max_no_progress_turns.saturating_mul(2)
    }

    pub fn block(&mut self, scope: Vec<String>, dependencies_sha256: String) {
        if self.watch.recovery != Recovery::Blocked {
            return;
        }
        let latest = ExecutionBlocker {
            scope,
            dependencies_sha256,
            watch: self.watch.clone(),
        };
        if let Some(existing) = self.blockers.iter_mut().find(|b| b.scope == latest.scope) {
            // A failed retry must block the dependencies it just tried. Keeping
            // the old digest would allow the same retry indefinitely.
            // Later handoff navigation on unchanged dependencies does not
            // replace the original exhausted scope's watch with handoff ticks.
            if existing.dependencies_sha256 != latest.dependencies_sha256 {
                *existing = latest;
            }
        } else {
            self.blockers.push(latest);
        }
    }

    /// The domain must reject unchanged blocked dependencies before calling.
    /// A retry gets one bounded attempt, retaining spent replan allowance.
    pub fn resume(&mut self, prior: Option<&ProgressWatch>) {
        self.watch = ProgressWatch {
            replans: prior.map_or(0, |w| w.replans),
            ..Default::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alternating_queries_and_restart_cannot_reset_recovery_allowance() {
        let limits = ProgressLimits {
            max_no_progress_turns: 2,
            max_focus_turns: 12,
            max_focus_replans: 1,
        };
        let mut state = Progress::default();
        state.observe(["version A".into(), "version B".into()], None, &limits);
        for value in ["version A", "version B"] {
            state.observe([value.into()], None, &limits);
        }
        assert_eq!(state.watch.recovery, Recovery::Replan);
        let mut restored: Progress = serde_json::from_value(serde_json::json!(state)).unwrap();
        for value in ["version A", "version B"] {
            restored.observe([value.into()], None, &limits);
        }
        assert_eq!(restored.watch.recovery, Recovery::Blocked);
        assert_eq!(restored.watch.replans, 1);
        assert!(!restored.watch.needs_replan_context());
    }
    #[test]
    fn new_evidence_cannot_extend_a_focus_forever_or_repeat_a_completion() {
        let limits = ProgressLimits {
            max_no_progress_turns: 2,
            max_focus_turns: 3,
            max_focus_replans: 1,
        };
        let mut state = Progress::default();
        for n in 0..3 {
            state.observe([n.to_string()], None, &limits);
        }
        assert_eq!(state.watch.recovery, Recovery::Replan);
        assert!(state.watch.needs_replan_context());
        state.observe(["another delivered page".into()], None, &limits);
        assert_eq!(state.watch.recovery, Recovery::Running);
        assert!(state.watch.needs_replan_context());
        state.observe(
            ["written".into()],
            Some("validated handoff".into()),
            &limits,
        );
        assert_eq!(state.watch.focus_turns, 0);
        assert!(!state.watch.needs_replan_context());
        for _ in 0..2 {
            state.observe(
                ["written".into()],
                Some("validated handoff".into()),
                &limits,
            );
        }
        assert_eq!(state.watch.recovery, Recovery::Replan);
    }
    #[test]
    fn recorded_twenty_eight_unproductive_turns_are_bounded() {
        let mut state = Progress::default();
        state.observe(
            ["already delivered source and candidate versions".into()],
            None,
            &ProgressLimits::default(),
        );
        for _ in 0..28 {
            state.observe(
                ["already delivered source and candidate versions".into()],
                None,
                &ProgressLimits::default(),
            );
        }
        assert_eq!(state.watch.recovery, Recovery::Blocked);
        assert_eq!(state.watch.replans, 2);
        assert!(state.handoff_exhausted(&ProgressLimits::default()));
    }

    #[test]
    fn a_failed_retry_blocks_the_latest_dependencies_without_refunding_replans() {
        let scope = vec!["source".into()];
        let mut state = Progress {
            watch: ProgressWatch {
                replans: 2,
                recovery: Recovery::Blocked,
                ..Default::default()
            },
            ..Default::default()
        };
        state.block(scope.clone(), "original dependencies".into());
        let original = state.blockers[0].watch.clone();
        state.resume(Some(&original));
        assert_eq!(state.watch.replans, 2);
        for _ in 0..ProgressLimits::default().max_no_progress_turns {
            state.observe([], None, &ProgressLimits::default());
        }
        assert_eq!(state.watch.recovery, Recovery::Blocked);
        state.block(scope.clone(), "changed dependencies".into());
        let blocked_ticks = state.blockers[0].watch.no_progress_turns;
        state.observe([], None, &ProgressLimits::default());
        assert!(state.watch.no_progress_turns > blocked_ticks);
        state.block(scope, "changed dependencies".into());
        assert_eq!(state.blockers[0].watch.no_progress_turns, blocked_ticks);
        let restored: Progress = serde_json::from_value(serde_json::json!(state)).unwrap();
        assert_eq!(restored.blockers.len(), 1);
        assert_eq!(
            restored.blockers[0].dependencies_sha256,
            "changed dependencies"
        );
        assert_eq!(restored.blockers[0].watch.replans, original.replans);
        assert_eq!(restored.blockers[0].watch.recovery, Recovery::Blocked);
    }
}
