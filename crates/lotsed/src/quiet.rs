//! Beta (wp09): the 1.5 s quiet window that holds, drops and collapses notifications.
//!
//! A status that flips back within the window (the agent was auto-approved, the user answered in the
//! terminal) is no reason to look up, so a notification waits [`HOLD`] before it goes out. It is dropped
//! when the session left the status that caused it, and repeats of one kind collapse into one.
//! Pure state over a caller-supplied clock: the caller feeds it events and status changes and asks
//! for the due ones.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use lotse_proto::{Event, NotificationKind, SessionId, Status};

/// How long a notification is held back.
pub const HOLD: Duration = Duration::from_millis(1500);

struct Held {
    session: SessionId,
    kind: NotificationKind,
    due: Instant,
    event: Event,
}

#[derive(Default)]
pub struct Quiet {
    held: Vec<Held>,
    /// The last status seen per session: a trigger can reach [`Quiet::hold`] after the status moved on.
    status: HashMap<SessionId, Status>,
}

/// The session status a notification is about; `None`: it stands on its own.
fn needs(kind: NotificationKind) -> Option<Status> {
    match kind {
        NotificationKind::Perm => Some(Status::Perm),
        NotificationKind::Fail => Some(Status::Fail),
        NotificationKind::Done => Some(Status::Done),
        NotificationKind::SubAgentDone => None,
    }
}

impl Quiet {
    /// Hold a `Event::Notification` for [`HOLD`]. Anything else is ignored.
    /// A repeat of a held (session, kind) replaces the event and keeps the first deadline, so a
    /// program that flips its status in a loop cannot postpone the notification for ever.
    pub fn hold(&mut self, now: Instant, event: Event) {
        let Event::Notification {
            session_id, kind, ..
        } = &event
        else {
            return;
        };
        let (session, kind) = (session_id.clone(), *kind);
        // the session may have left the status before its trigger got here
        if let (Some(&known), Some(needed)) = (self.status.get(&session), needs(kind))
            && known != needed
        {
            return;
        }
        match self
            .held
            .iter_mut()
            .find(|h| h.session == session && h.kind == kind)
        {
            Some(h) => h.event = event,
            None => self.held.push(Held {
                session,
                kind,
                due: now + HOLD,
                event,
            }),
        }
    }

    /// The session's status is now `status`: drop what it no longer applies to.
    pub fn status_changed(&mut self, session: &SessionId, status: Status) {
        self.status.insert(session.clone(), status);
        self.held
            .retain(|h| h.session != *session || needs(h.kind).is_none_or(|s| s == status));
    }

    /// The session is gone: forget its status and what is held for it.
    pub fn forget(&mut self, session: &SessionId) {
        self.status.remove(session);
        self.held.retain(|h| h.session != *session);
    }

    /// The held events that are due, oldest first.
    pub fn release(&mut self, now: Instant) -> Vec<Event> {
        let (mut due, held): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|h| h.due <= now);
        self.held = held;
        due.sort_by_key(|h| h.due);
        due.into_iter().map(|h| h.event).collect()
    }

    /// When the next held event is due, to arm a timer.
    pub fn next_due(&self) -> Option<Instant> {
        self.held.iter().map(|h| h.due).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(session: &str, kind: NotificationKind, prompt: &str) -> Event {
        Event::Notification {
            session_id: SessionId::new(session),
            kind,
            title: "p · Claude Code".into(),
            subtitle: "Needs permission".into(),
            body: None,
            banner: true,
            sound: true,
            sound_name: None,
            prompt_id: Some(prompt.into()),
            at: 1,
        }
    }

    fn prompt_of(event: &Event) -> Option<&str> {
        match event {
            Event::Notification { prompt_id, .. } => prompt_id.as_deref(),
            _ => None,
        }
    }

    fn after(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn a_held_event_goes_out_when_the_window_is_over() {
        let t0 = Instant::now();
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Perm, "p-1"));
        assert_eq!(q.next_due(), Some(after(t0, 1500)));
        assert!(q.release(after(t0, 1499)).is_empty());
        let out = q.release(after(t0, 1500));
        assert_eq!(out.len(), 1);
        assert_eq!(prompt_of(&out[0]), Some("p-1"));
        assert!(q.release(after(t0, 5000)).is_empty(), "released once");
        assert_eq!(q.next_due(), None);
    }

    #[test]
    fn perm_flip_within_window_is_dropped() {
        let t0 = Instant::now();
        let a = SessionId::new("a");
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Perm, "a-1"));
        q.hold(t0, note("b", NotificationKind::Perm, "b-1"));
        // a's agent carries on (auto-approved, or answered in the terminal); b still waits
        q.status_changed(&a, Status::Run);
        let out = q.release(after(t0, 2000));
        assert_eq!(
            out.iter().filter_map(prompt_of).collect::<Vec<_>>(),
            ["b-1"]
        );
        assert_eq!(q.next_due(), None);
    }

    #[test]
    fn a_flip_in_another_session_or_to_the_same_status_keeps_it() {
        let t0 = Instant::now();
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Perm, "p-1"));
        q.status_changed(&SessionId::new("b"), Status::Run);
        q.status_changed(&SessionId::new("a"), Status::Perm);
        assert_eq!(q.release(after(t0, 1500)).len(), 1);
    }

    #[test]
    fn a_new_prompt_after_a_flip_is_held_on_its_own() {
        let t0 = Instant::now();
        let a = SessionId::new("a");
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Perm, "p-1"));
        q.status_changed(&a, Status::Run);
        q.status_changed(&a, Status::Perm);
        q.hold(after(t0, 400), note("a", NotificationKind::Perm, "p-2"));
        let out = q.release(after(t0, 1900));
        assert_eq!(out.len(), 1);
        assert_eq!(prompt_of(&out[0]), Some("p-2"));
    }

    #[test]
    fn repeats_collapse() {
        let t0 = Instant::now();
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Perm, "p-1"));
        q.hold(after(t0, 300), note("a", NotificationKind::Perm, "p-2"));
        q.hold(after(t0, 900), note("a", NotificationKind::Perm, "p-3"));
        // one per session and kind, the latest, and the first one's deadline stands
        let out = q.release(after(t0, 1500));
        assert_eq!(out.len(), 1);
        assert_eq!(prompt_of(&out[0]), Some("p-3"));
        assert!(q.release(after(t0, 3000)).is_empty());
    }

    #[test]
    fn other_sessions_and_kinds_do_not_collapse_and_go_out_oldest_first() {
        let t0 = Instant::now();
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Perm, "a-perm"));
        q.hold(after(t0, 100), note("b", NotificationKind::Perm, "b-perm"));
        q.hold(after(t0, 200), note("a", NotificationKind::Fail, "a-fail"));
        let out = q.release(after(t0, 2000));
        let ids: Vec<_> = out.iter().filter_map(prompt_of).collect();
        assert_eq!(ids, ["a-perm", "b-perm", "a-fail"]);
    }

    #[test]
    fn a_sub_agent_notice_does_not_depend_on_the_session_status() {
        let t0 = Instant::now();
        let a = SessionId::new("a");
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::SubAgentDone, "sub"));
        q.status_changed(&a, Status::Run);
        assert_eq!(q.release(after(t0, 1500)).len(), 1);
    }

    #[test]
    fn a_done_that_became_a_run_again_is_dropped() {
        let t0 = Instant::now();
        let a = SessionId::new("a");
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Done, "d"));
        q.status_changed(&a, Status::Run);
        assert!(q.release(after(t0, 1500)).is_empty());
    }

    #[test]
    fn a_notification_for_a_status_already_left_is_not_held() {
        let t0 = Instant::now();
        let a = SessionId::new("a");
        let mut q = Quiet::default();
        // the status moved on before the trigger got here
        q.status_changed(&a, Status::Run);
        q.hold(t0, note("a", NotificationKind::Perm, "p-1"));
        assert_eq!(q.next_due(), None);
        q.status_changed(&a, Status::Perm);
        q.hold(t0, note("a", NotificationKind::Perm, "p-2"));
        assert_eq!(q.release(after(t0, 1500)).len(), 1);
    }

    #[test]
    fn a_closed_session_is_forgotten() {
        let t0 = Instant::now();
        let a = SessionId::new("a");
        let mut q = Quiet::default();
        q.hold(t0, note("a", NotificationKind::Perm, "p-1"));
        q.status_changed(&a, Status::Perm);
        q.forget(&a);
        assert!(q.release(after(t0, 2000)).is_empty());
        q.hold(t0, note("a", NotificationKind::Perm, "p-2"));
        assert_eq!(q.next_due(), Some(after(t0, 1500)), "no stale status left");
    }

    #[test]
    fn other_events_are_not_held() {
        let t0 = Instant::now();
        let mut q = Quiet::default();
        q.hold(
            t0,
            Event::SessionClosed {
                session_id: SessionId::new("a"),
            },
        );
        assert_eq!(q.next_due(), None);
    }
}
