//! Status engine. Public API is frozen (WP0); WP2 implements the bodies.
//!
//! One `Engine` per session: owns the registered adapters, merges their detections by source
//! precedence, and reports a [`StatusUpdate`] only when status, sub-agents or the permission
//! request actually change.
//!
//! Merge rule: the engine remembers the last opinion of every [`DetectSource`]. The merged status is
//! the opinion of the strongest source (`Hook` > `Osc7501` > `Screen` > `Process`) that has one;
//! `Status::Unknown` is "no signal" and never wins. A weaker source therefore cannot override a
//! stronger one: a hook `perm` stays until the hook adapter clears it. Sub-agents and the permission
//! request come from the strongest source that reported them / is the winning source.

use crate::adapter::{Adapter, Detection, Observation};
use crate::{claude, opencode, osc7501, screen};
use lotse_proto::{AgentKind, DetectSource, PermRequest, SessionId, Status, SubAgent, UnixMs};
use std::collections::BTreeMap;

/// A merged change, ready to become `Event::StatusChanged` / `Event::SubAgentsChanged`.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusUpdate {
    pub status: Status,
    pub source: DetectSource,
    pub perm: Option<PermRequest>,
    /// `Some` only when the sub-agent list changed.
    pub sub_agents: Option<Vec<SubAgent>>,
    /// Set while `status == Run`.
    pub running_since: Option<UnixMs>,
    pub at: UnixMs,
}

/// Last opinion of one source.
#[derive(Debug, Clone, Default)]
struct SourceState {
    status: Option<Status>,
    perm: Option<PermRequest>,
    sub_agents: Option<Vec<SubAgent>>,
}

/// The merged view the engine reports from.
#[derive(Debug, Clone)]
struct Merged {
    status: Status,
    perm: Option<PermRequest>,
    sub_agents: Vec<SubAgent>,
    running_since: Option<UnixMs>,
}

impl Merged {
    fn initial() -> Self {
        Self {
            status: Status::Unknown,
            perm: None,
            sub_agents: Vec::new(),
            running_since: None,
        }
    }
}

pub struct Engine {
    session_id: SessionId,
    agent: AgentKind,
    adapters: Vec<Box<dyn Adapter>>,
    /// Ordered by precedence (`DetectSource` derives `Ord`, strongest first).
    sources: BTreeMap<DetectSource, SourceState>,
    merged: Merged,
}

impl Engine {
    /// Engine with no adapters; status stays `Unknown`.
    pub fn new(session_id: SessionId, agent: AgentKind) -> Self {
        Self {
            session_id,
            agent,
            adapters: Vec::new(),
            sources: BTreeMap::new(),
            merged: Merged::initial(),
        }
    }

    /// Engine with the bundled adapters for `agent`: registers whatever `claude::`, `osc7501::`,
    /// `screen::` and `opencode::default_adapter(&session_id, agent)` return (each module owns its
    /// constructor, so WP6 never edits this file).
    pub fn with_default_adapters(session_id: SessionId, agent: AgentKind) -> Self {
        let mut engine = Self::new(session_id.clone(), agent);
        let adapters = [
            claude::default_adapter(&session_id, agent),
            osc7501::default_adapter(&session_id, agent),
            screen::default_adapter(&session_id, agent),
            opencode::default_adapter(&session_id, agent),
        ];
        for adapter in adapters.into_iter().flatten() {
            engine.register(adapter);
        }
        engine
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn agent(&self) -> AgentKind {
        self.agent
    }

    pub fn register(&mut self, adapter: Box<dyn Adapter>) {
        self.adapters.push(adapter);
    }

    /// Offer an observation to every adapter and merge; `None` when nothing changed.
    pub fn observe(&mut self, obs: Observation) -> Option<StatusUpdate> {
        let detections: Vec<Detection> = self
            .adapters
            .iter_mut()
            .flat_map(|adapter| adapter.observe(&obs))
            .collect();
        let mut at = None;
        for detection in detections {
            at = Some(detection.at);
            self.apply(detection);
        }
        self.merge(at?)
    }

    /// Current merged status.
    pub fn status(&self) -> Status {
        self.merged.status
    }

    /// Current sub-agents.
    pub fn sub_agents(&self) -> &[SubAgent] {
        &self.merged.sub_agents
    }

    /// Record one detection as the latest opinion of its source.
    fn apply(&mut self, detection: Detection) {
        let state = self.sources.entry(detection.source).or_default();
        if let Some(status) = detection.status {
            state.status = Some(status);
            // `perm` only travels with `Status::Perm`; any other status clears it.
            state.perm = if status == Status::Perm {
                detection.perm
            } else {
                None
            };
        }
        if let Some(sub_agents) = detection.sub_agents {
            state.sub_agents = Some(sub_agents);
        }
    }

    /// Recompute the merged view; `Some` when status, perm or sub-agents differ from before.
    fn merge(&mut self, at: UnixMs) -> Option<StatusUpdate> {
        let winner = self
            .sources
            .iter()
            .find(|(_, state)| matches!(state.status, Some(s) if s != Status::Unknown));
        let (status, source, perm) = match winner {
            Some((source, state)) => (
                state.status.unwrap_or(Status::Unknown),
                *source,
                state.perm.clone(),
            ),
            None => (Status::Unknown, DetectSource::Process, None),
        };
        let sub_agents = self
            .sources
            .values()
            .find_map(|state| state.sub_agents.clone())
            .unwrap_or_default();

        let status_changed = status != self.merged.status;
        let perm_changed = perm != self.merged.perm;
        let subs_changed = sub_agents != self.merged.sub_agents;
        let running_since = match status {
            Status::Run if self.merged.status == Status::Run => self.merged.running_since,
            Status::Run => Some(at),
            _ => None,
        };

        self.merged = Merged {
            status,
            perm: perm.clone(),
            sub_agents: sub_agents.clone(),
            running_since,
        };

        (status_changed || perm_changed || subs_changed).then_some(StatusUpdate {
            status,
            source,
            perm,
            sub_agents: subs_changed.then_some(sub_agents),
            running_since,
            at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lotse_proto::SubAgentId;

    /// Adapter that answers every observation with the next scripted detection list.
    struct Scripted {
        source: DetectSource,
        script: Vec<Vec<Detection>>,
    }

    impl Adapter for Scripted {
        fn name(&self) -> &'static str {
            "scripted"
        }
        fn agent(&self) -> AgentKind {
            AgentKind::ClaudeCode
        }
        fn source(&self) -> DetectSource {
            self.source
        }
        fn observe(&mut self, _obs: &Observation) -> Vec<Detection> {
            if self.script.is_empty() {
                Vec::new()
            } else {
                self.script.remove(0)
            }
        }
    }

    fn det(source: DetectSource, status: Status, at: UnixMs) -> Detection {
        Detection {
            source,
            status: Some(status),
            sub_agents: None,
            perm: None,
            at,
        }
    }

    fn perm_det(source: DetectSource, at: UnixMs) -> Detection {
        Detection {
            perm: Some(PermRequest {
                tool: "Bash".into(),
                summary: "ls".into(),
            }),
            ..det(source, Status::Perm, at)
        }
    }

    fn subs_det(source: DetectSource, subs: &[(&str, Status)], at: UnixMs) -> Detection {
        Detection {
            source,
            status: None,
            sub_agents: Some(
                subs.iter()
                    .map(|(id, status)| SubAgent {
                        id: SubAgentId::new(*id),
                        name: (*id).into(),
                        status: *status,
                    })
                    .collect(),
            ),
            perm: None,
            at,
        }
    }

    fn tick(at: UnixMs) -> Observation {
        Observation::Tick { now: at }
    }

    /// Engine fed one scripted source per entry; every `observe` call consumes one step of each.
    fn engine_with(scripts: Vec<(DetectSource, Vec<Vec<Detection>>)>) -> Engine {
        let mut e = Engine::new(SessionId::new("a1"), AgentKind::ClaudeCode);
        for (source, script) in scripts {
            e.register(Box::new(Scripted { source, script }));
        }
        e
    }

    #[test]
    fn new_engine_starts_unknown() {
        let e = Engine::new(SessionId::new("a1"), AgentKind::ClaudeCode);
        assert_eq!(e.status(), Status::Unknown);
        assert!(e.sub_agents().is_empty());
        assert_eq!(e.session_id().as_str(), "a1");
        assert_eq!(e.agent(), AgentKind::ClaudeCode);
    }

    #[test]
    fn no_adapters_means_no_updates() {
        let mut e = Engine::new(SessionId::new("a1"), AgentKind::Terminal);
        assert_eq!(e.observe(tick(1)), None);
        assert_eq!(e.status(), Status::Unknown);
    }

    #[test]
    fn default_adapters_for_claude_include_the_hook_adapter() {
        let mut e = Engine::with_default_adapters(SessionId::new("a1"), AgentKind::ClaudeCode);
        let up = e
            .observe(Observation::Hook(crate::HookObservation {
                agent: AgentKind::ClaudeCode,
                payload: serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
                at: 5,
            }))
            .expect("hook adapter registered");
        assert_eq!(up.status, Status::Run);
        assert_eq!(up.source, DetectSource::Hook);
        // A plain terminal gets no hook adapter.
        let mut t = Engine::with_default_adapters(SessionId::new("a2"), AgentKind::Terminal);
        assert_eq!(
            t.observe(Observation::Hook(crate::HookObservation {
                agent: AgentKind::ClaudeCode,
                payload: serde_json::json!({"hook_event_name": "UserPromptSubmit"}),
                at: 5,
            })),
            None
        );
    }

    #[test]
    fn stronger_source_wins_over_weaker() {
        // hook says perm, a later screen detection says run: status stays perm until the hook clears it.
        let mut e = engine_with(vec![
            (
                DetectSource::Hook,
                vec![
                    vec![perm_det(DetectSource::Hook, 1)],
                    vec![],
                    vec![det(DetectSource::Hook, Status::Run, 3)],
                ],
            ),
            (
                DetectSource::Screen,
                vec![
                    vec![],
                    vec![det(DetectSource::Screen, Status::Run, 2)],
                    vec![],
                ],
            ),
        ]);
        let up = e.observe(tick(1)).unwrap();
        assert_eq!((up.status, up.source), (Status::Perm, DetectSource::Hook));
        assert!(up.perm.is_some());

        // Screen says run: stronger hook perm stays, nothing to report.
        assert_eq!(e.observe(tick(2)), None);
        assert_eq!(e.status(), Status::Perm);

        // Hook clears perm: now run (the screen agrees), perm gone.
        let up = e.observe(tick(3)).unwrap();
        assert_eq!(up.status, Status::Run);
        assert_eq!(up.perm, None);
    }

    #[test]
    fn weaker_source_is_used_when_the_stronger_has_no_signal() {
        let mut e = engine_with(vec![
            (
                DetectSource::Hook,
                vec![vec![det(DetectSource::Hook, Status::Unknown, 1)], vec![]],
            ),
            (
                DetectSource::Screen,
                vec![vec![], vec![det(DetectSource::Screen, Status::Run, 2)]],
            ),
        ]);
        assert_eq!(e.observe(tick(1)), None, "unknown is no signal");
        let up = e.observe(tick(2)).unwrap();
        assert_eq!((up.status, up.source), (Status::Run, DetectSource::Screen));
    }

    #[test]
    fn precedence_is_hook_osc_screen_process() {
        let all = [
            DetectSource::Process,
            DetectSource::Screen,
            DetectSource::Osc7501,
            DetectSource::Hook,
        ];
        // Each step adds a stronger source with a distinct status; the strongest present always wins.
        let statuses = [Status::Idle, Status::Run, Status::Fail, Status::Done];
        let mut e = Engine::new(SessionId::new("a1"), AgentKind::ClaudeCode);
        let script: Vec<Vec<Detection>> = all
            .iter()
            .zip(statuses)
            .map(|(s, st)| vec![det(*s, st, 1)])
            .collect();
        for (i, step) in script.into_iter().enumerate() {
            e.register(Box::new(Scripted {
                source: all[i],
                script: vec![step],
            }));
            let up = e.observe(tick(1)).unwrap();
            assert_eq!(up.source, all[i]);
            assert_eq!(up.status, statuses[i]);
        }
    }

    #[test]
    fn unchanged_detection_reports_nothing() {
        let mut e = engine_with(vec![(
            DetectSource::Hook,
            vec![
                vec![det(DetectSource::Hook, Status::Run, 1)],
                vec![det(DetectSource::Hook, Status::Run, 2)],
            ],
        )]);
        assert!(e.observe(tick(1)).is_some());
        assert_eq!(e.observe(tick(2)), None);
    }

    #[test]
    fn running_since_is_set_on_entering_run_and_kept_while_running() {
        let mut e = engine_with(vec![(
            DetectSource::Hook,
            vec![
                vec![det(DetectSource::Hook, Status::Run, 100)],
                // Perm request while running, then back to run: a new run period starts.
                vec![perm_det(DetectSource::Hook, 200)],
                vec![det(DetectSource::Hook, Status::Run, 300)],
                vec![det(DetectSource::Hook, Status::Done, 400)],
            ],
        )]);
        assert_eq!(e.observe(tick(100)).unwrap().running_since, Some(100));
        assert_eq!(e.observe(tick(200)).unwrap().running_since, None);
        assert_eq!(e.observe(tick(300)).unwrap().running_since, Some(300));
        assert_eq!(e.observe(tick(400)).unwrap().running_since, None);
    }

    #[test]
    fn sub_agent_changes_are_reported_even_when_status_is_unchanged() {
        let mut e = engine_with(vec![(
            DetectSource::Hook,
            vec![
                vec![det(DetectSource::Hook, Status::Run, 1)],
                vec![subs_det(DetectSource::Hook, &[("s1", Status::Run)], 2)],
                vec![subs_det(DetectSource::Hook, &[("s1", Status::Run)], 3)],
                vec![subs_det(DetectSource::Hook, &[("s1", Status::Done)], 4)],
                vec![subs_det(DetectSource::Hook, &[], 5)],
            ],
        )]);
        assert!(e.observe(tick(1)).unwrap().sub_agents.is_none());
        let up = e.observe(tick(2)).unwrap();
        assert_eq!(up.status, Status::Run);
        assert_eq!(up.sub_agents.as_ref().map(Vec::len), Some(1));
        assert_eq!(e.observe(tick(3)), None, "identical list is not a change");
        let up = e.observe(tick(4)).unwrap();
        assert_eq!(up.sub_agents.unwrap()[0].status, Status::Done);
        assert_eq!(e.sub_agents().len(), 1);
        let up = e.observe(tick(5)).unwrap();
        assert_eq!(up.sub_agents, Some(vec![]));
        assert!(e.sub_agents().is_empty());
    }

    #[test]
    fn perm_request_change_alone_is_reported() {
        let mut first = perm_det(DetectSource::Hook, 1);
        let mut second = perm_det(DetectSource::Hook, 2);
        first.perm.as_mut().unwrap().summary = "ls".into();
        second.perm.as_mut().unwrap().summary = "rm -rf build".into();
        let mut e = engine_with(vec![(DetectSource::Hook, vec![vec![first], vec![second]])]);
        e.observe(tick(1)).unwrap();
        let up = e.observe(tick(2)).unwrap();
        assert_eq!(up.status, Status::Perm);
        assert_eq!(up.perm.unwrap().summary, "rm -rf build");
    }
}
