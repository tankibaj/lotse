//! Native notifications and the Dock badge (WP7).
//!
//! Two layers:
//! - **Decision logic** (`resolve`, `gate`, `decide`): pure functions over the protocol's
//!   settings, mirroring the mock's `notifies(project, event)` plus the pause bell (⌥⌘N) and
//!   "Only when Lotse is not focused on that session". Unit-tested below.
//! - **Tauri adapter** (`NotifyState`, `on_event`, `set_badge`, `set_focused_session`): shows the
//!   macOS notification and sets the Dock badge. WP8 wires it into the entry point (`lib.rs`):
//!   see the Requests in `status/WP7.md`.
//!
//! The daemon already applies Settings and the per-project override before it emits
//! `Event::Notification` ("settings and per-project overrides applied"), so [`on_event`] only
//! applies what the daemon cannot know: the pause bell as the app last saw it and window focus.
//! [`resolve`] / [`decide`] cover the full path for callers that start from a raw status change.

use std::sync::{Mutex, MutexGuard, PoisonError};

use lotse_proto::{Event, NotificationKind, NotificationMode, NotificationSettings, SessionId};
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_notification::NotificationExt;

/// macOS system sound used by the sound-only path (banner off, sound on).
const SOUND_FILE: &str = "/System/Library/Sounds/Ping.aiff";

// --- decision logic -------------------------------------------------------------------------

/// What to deliver for one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub banner: bool,
    pub sound: bool,
}

/// Why nothing is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Footer bell / ⌥⌘N: all notifications paused.
    Paused,
    /// Project override "Mute this project".
    Muted,
    /// The event's toggle is off (global row, or the project's custom choice).
    Off,
    /// Lotse is focused on that session and "only when unfocused" is on.
    Focused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Deliver(Delivery),
    Skip(Skip),
}

/// What the user is looking at right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    /// The Lotse window is the key window.
    pub app_focused: bool,
    /// The session shown in the focused pane.
    pub session: Option<SessionId>,
}

fn prefs(settings: &NotificationSettings, kind: NotificationKind) -> lotse_proto::EventPrefs {
    match kind {
        NotificationKind::Perm => settings.perm.clone(),
        NotificationKind::Fail => settings.fail.clone(),
        NotificationKind::Done => settings.done.clone(),
        NotificationKind::SubAgentDone => settings.sub_agent_done.clone(),
    }
}

/// Settings + per-project override → what to deliver (mock `notifies`).
/// `Custom` replaces the banner toggle per event; the sound still follows the global row.
/// Does not look at pause or focus: see [`gate`].
pub fn resolve(
    settings: &NotificationSettings,
    mode: &NotificationMode,
    kind: NotificationKind,
) -> Result<Delivery, Skip> {
    let global = prefs(settings, kind);
    let banner = match mode {
        NotificationMode::Mute => return Err(Skip::Muted),
        NotificationMode::Global => global.banner,
        NotificationMode::Custom {
            perm,
            fail,
            done,
            sub_agent_done,
        } => match kind {
            NotificationKind::Perm => *perm,
            NotificationKind::Fail => *fail,
            NotificationKind::Done => *done,
            NotificationKind::SubAgentDone => *sub_agent_done,
        },
    };
    // A custom "off" silences the whole event, sound included.
    let sound = global.sound && (banner || matches!(mode, NotificationMode::Global));
    if banner || sound {
        Ok(Delivery { banner, sound })
    } else {
        Err(Skip::Off)
    }
}

/// The checks only the app can make: pause bell and focus.
pub fn gate(
    settings: &NotificationSettings,
    session: &SessionId,
    focus: &Focus,
) -> Result<(), Skip> {
    if settings.paused {
        return Err(Skip::Paused);
    }
    if settings.only_when_unfocused && focus.app_focused && focus.session.as_ref() == Some(session)
    {
        return Err(Skip::Focused);
    }
    Ok(())
}

/// Full decision: pause, then project override and toggles, then focus.
pub fn decide(
    settings: &NotificationSettings,
    mode: &NotificationMode,
    kind: NotificationKind,
    session: &SessionId,
    focus: &Focus,
) -> Decision {
    if settings.paused {
        return Decision::Skip(Skip::Paused);
    }
    match resolve(settings, mode, kind).and_then(|d| gate(settings, session, focus).map(|()| d)) {
        Ok(d) => Decision::Deliver(d),
        Err(s) => Decision::Skip(s),
    }
}

/// Decision for a daemon `Event::Notification` (already resolved: banner on, `sound` from the event).
pub fn decide_event(
    settings: &NotificationSettings,
    session: &SessionId,
    sound: bool,
    focus: &Focus,
) -> Decision {
    match gate(settings, session, focus) {
        Ok(()) => Decision::Deliver(Delivery {
            banner: true,
            sound,
        }),
        Err(s) => Decision::Skip(s),
    }
}

/// Banner text. The plugin has title + body only: the mock's second and third line share the body.
pub fn banner_body(subtitle: &str, body: Option<&str>) -> String {
    match body.filter(|b| !b.is_empty()) {
        Some(b) => format!("{subtitle}\n{b}"),
        None => subtitle.to_string(),
    }
}

// --- Tauri adapter ----------------------------------------------------------------------------

/// What the app last learned from the daemon and the webview. Managed Tauri state.
#[derive(Default)]
pub struct NotifyState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    settings: NotificationSettings,
    focused_session: Option<SessionId>,
}

impl NotifyState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Call with `Snapshot.settings.notifications` on connect and on every `SettingsChanged`.
    pub fn set_settings(&self, settings: NotificationSettings) {
        self.lock().settings = settings;
    }

    pub fn set_focused_session(&self, session: Option<SessionId>) {
        self.lock().focused_session = session;
    }

    fn snapshot(&self) -> (NotificationSettings, Option<SessionId>) {
        let g = self.lock();
        (g.settings.clone(), g.focused_session.clone())
    }
}

fn app_focused<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.webview_windows()
        .values()
        .any(|w| w.is_focused().unwrap_or(false))
}

/// Feed every daemon event here. Handles `SettingsChanged` (keeps the pause bell current) and
/// `Notification` (shows the banner unless paused or focused). Other events are ignored.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: &Event) {
    let Some(state) = app.try_state::<NotifyState>() else {
        tracing::warn!("notify: NotifyState is not managed; event dropped");
        return;
    };
    match event {
        Event::SettingsChanged(s) => state.set_settings(s.notifications.clone()),
        Event::Notification {
            session_id,
            title,
            subtitle,
            body,
            sound,
            ..
        } => {
            let (settings, focused_session) = state.snapshot();
            let focus = Focus {
                app_focused: app_focused(app),
                session: focused_session,
            };
            match decide_event(&settings, session_id, *sound, &focus) {
                Decision::Deliver(d) => {
                    if let Err(e) = deliver(app, d, title, &banner_body(subtitle, body.as_deref()))
                    {
                        tracing::warn!("notify: delivery failed: {e}");
                    }
                }
                Decision::Skip(why) => tracing::debug!("notify: skipped for {session_id}: {why:?}"),
            }
        }
        _ => {}
    }
}

fn deliver<R: Runtime>(
    app: &AppHandle<R>,
    d: Delivery,
    title: &str,
    body: &str,
) -> Result<(), String> {
    if d.banner {
        let mut n = app.notification().builder().title(title).body(body);
        if d.sound {
            // Unverified: "Default" is passed through to mac-notification-sys as the system default sound.
            n = n.sound("Default");
        }
        n.show().map_err(|e| e.to_string())
    } else if d.sound {
        // Sound without banner: no plugin API for it. A missing file is harmless. Not reachable from
        // `on_event` today (the daemon event always means a banner); kept for `decide` callers.
        let mut child = std::process::Command::new("afplay")
            .arg(SOUND_FILE)
            .spawn()
            .map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            let _ = child.wait(); // reap, no zombie
        });
        Ok(())
    } else {
        Ok(())
    }
}

/// Dock badge = need-you count; 0 clears it. The webview calls this from
/// `subscribeNeedYouCount` (`app/src/lib/state`).
#[tauri::command]
pub fn set_badge<R: Runtime>(app: AppHandle<R>, count: u32) -> Result<(), String> {
    let window = app
        .webview_windows()
        .into_values()
        .next()
        .ok_or("no window")?;
    window
        .set_badge_count((count > 0).then_some(i64::from(count)))
        .map_err(|e| e.to_string())
}

/// The webview reports the session shown in the focused pane (`None`: no session shown).
#[tauri::command]
pub fn set_focused_session(state: State<'_, NotifyState>, session_id: Option<String>) {
    state.set_focused_session(session_id.map(SessionId::new));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds() -> [NotificationKind; 4] {
        [
            NotificationKind::Perm,
            NotificationKind::Fail,
            NotificationKind::Done,
            NotificationKind::SubAgentDone,
        ]
    }

    fn sid(s: &str) -> SessionId {
        SessionId::new(s)
    }

    fn unfocused() -> Focus {
        Focus::default()
    }

    fn deliver_of(d: Decision) -> Delivery {
        match d {
            Decision::Deliver(d) => d,
            Decision::Skip(s) => panic!("expected delivery, skipped: {s:?}"),
        }
    }

    #[test]
    fn global_defaults_match_the_mock_table() {
        // Mock defaults: perm + fail banner and sound, done banner only, sub-agent done off.
        let s = NotificationSettings::default();
        let g = NotificationMode::Global;
        assert_eq!(
            resolve(&s, &g, NotificationKind::Perm),
            Ok(Delivery {
                banner: true,
                sound: true
            })
        );
        assert_eq!(
            resolve(&s, &g, NotificationKind::Fail),
            Ok(Delivery {
                banner: true,
                sound: true
            })
        );
        assert_eq!(
            resolve(&s, &g, NotificationKind::Done),
            Ok(Delivery {
                banner: true,
                sound: false
            })
        );
        assert_eq!(
            resolve(&s, &g, NotificationKind::SubAgentDone),
            Err(Skip::Off)
        );
    }

    #[test]
    fn global_toggle_off_skips_only_that_event() {
        let mut s = NotificationSettings::default();
        s.fail.banner = false;
        s.fail.sound = false;
        assert_eq!(
            resolve(&s, &NotificationMode::Global, NotificationKind::Fail),
            Err(Skip::Off)
        );
        assert!(resolve(&s, &NotificationMode::Global, NotificationKind::Perm).is_ok());
    }

    #[test]
    fn sound_without_banner_is_sound_only() {
        let mut s = NotificationSettings::default();
        s.done.banner = false;
        s.done.sound = true;
        assert_eq!(
            resolve(&s, &NotificationMode::Global, NotificationKind::Done),
            Ok(Delivery {
                banner: false,
                sound: true
            })
        );
    }

    #[test]
    fn muted_project_never_notifies() {
        let s = NotificationSettings::default();
        for k in kinds() {
            assert_eq!(resolve(&s, &NotificationMode::Mute, k), Err(Skip::Muted));
        }
    }

    #[test]
    fn custom_mode_overrides_global_per_event() {
        let s = NotificationSettings::default(); // sub-agent done is off globally
        let m = NotificationMode::Custom {
            perm: false,
            fail: true,
            done: false,
            sub_agent_done: true,
        };
        assert_eq!(resolve(&s, &m, NotificationKind::Perm), Err(Skip::Off));
        // on in custom: banner shows, sound follows the global row (fail: on, sub-agent done: off)
        assert_eq!(
            resolve(&s, &m, NotificationKind::Fail),
            Ok(Delivery {
                banner: true,
                sound: true
            })
        );
        assert_eq!(
            resolve(&s, &m, NotificationKind::SubAgentDone),
            Ok(Delivery {
                banner: true,
                sound: false
            })
        );
        assert_eq!(resolve(&s, &m, NotificationKind::Done), Err(Skip::Off));
    }

    #[test]
    fn custom_off_silences_the_sound_too() {
        let s = NotificationSettings::default(); // perm sound on globally
        let m = NotificationMode::Custom {
            perm: false,
            fail: false,
            done: false,
            sub_agent_done: false,
        };
        assert_eq!(resolve(&s, &m, NotificationKind::Perm), Err(Skip::Off));
    }

    #[test]
    fn pause_bell_beats_everything() {
        let s = NotificationSettings {
            paused: true,
            ..Default::default()
        };
        for k in kinds() {
            for m in [NotificationMode::Global, NotificationMode::Mute] {
                assert_eq!(
                    decide(&s, &m, k, &sid("a"), &unfocused()),
                    Decision::Skip(Skip::Paused)
                );
            }
        }
        assert_eq!(
            decide_event(&s, &sid("a"), true, &unfocused()),
            Decision::Skip(Skip::Paused)
        );
    }

    #[test]
    fn unpausing_restores_delivery() {
        let mut s = NotificationSettings {
            paused: true,
            ..Default::default()
        };
        assert_eq!(
            decide_event(&s, &sid("a"), false, &unfocused()),
            Decision::Skip(Skip::Paused)
        );
        s.paused = false;
        assert!(matches!(
            decide_event(&s, &sid("a"), false, &unfocused()),
            Decision::Deliver(_)
        ));
    }

    #[test]
    fn focused_session_is_skipped_only_when_the_setting_is_on() {
        let mut s = NotificationSettings::default();
        let here = Focus {
            app_focused: true,
            session: Some(sid("a")),
        };
        assert_eq!(
            decide_event(&s, &sid("a"), true, &here),
            Decision::Skip(Skip::Focused)
        );
        s.only_when_unfocused = false;
        assert!(matches!(
            decide_event(&s, &sid("a"), true, &here),
            Decision::Deliver(_)
        ));
    }

    #[test]
    fn other_session_or_unfocused_window_still_notifies() {
        let s = NotificationSettings::default();
        let other = Focus {
            app_focused: true,
            session: Some(sid("b")),
        };
        assert!(matches!(
            decide_event(&s, &sid("a"), true, &other),
            Decision::Deliver(_)
        ));
        let backgrounded = Focus {
            app_focused: false,
            session: Some(sid("a")),
        };
        assert!(matches!(
            decide_event(&s, &sid("a"), true, &backgrounded),
            Decision::Deliver(_)
        ));
        let nothing_shown = Focus {
            app_focused: true,
            session: None,
        };
        assert!(matches!(
            decide_event(&s, &sid("a"), true, &nothing_shown),
            Decision::Deliver(_)
        ));
    }

    #[test]
    fn decide_combines_override_and_focus() {
        let s = NotificationSettings::default();
        let here = Focus {
            app_focused: true,
            session: Some(sid("a")),
        };
        // toggle on, but the user is looking at the session
        assert_eq!(
            decide(
                &s,
                &NotificationMode::Global,
                NotificationKind::Perm,
                &sid("a"),
                &here
            ),
            Decision::Skip(Skip::Focused)
        );
        // muted wins over everything else, with no focus involved
        assert_eq!(
            decide(
                &s,
                &NotificationMode::Mute,
                NotificationKind::Perm,
                &sid("a"),
                &unfocused()
            ),
            Decision::Skip(Skip::Muted)
        );
        let d = deliver_of(decide(
            &s,
            &NotificationMode::Global,
            NotificationKind::Perm,
            &sid("a"),
            &unfocused(),
        ));
        assert_eq!(
            d,
            Delivery {
                banner: true,
                sound: true
            }
        );
    }

    #[test]
    fn event_sound_flag_is_kept() {
        let s = NotificationSettings::default();
        assert_eq!(
            deliver_of(decide_event(&s, &sid("a"), false, &unfocused())),
            Delivery {
                banner: true,
                sound: false
            }
        );
        assert_eq!(
            deliver_of(decide_event(&s, &sid("a"), true, &unfocused())),
            Delivery {
                banner: true,
                sound: true
            }
        );
    }

    #[test]
    fn banner_body_joins_subtitle_and_body() {
        assert_eq!(
            banner_body("Needs permission", Some("Bash(npx playwright test)")),
            "Needs permission\nBash(npx playwright test)"
        );
        assert_eq!(banner_body("Done", None), "Done");
        assert_eq!(banner_body("Done", Some("")), "Done");
    }

    #[test]
    fn state_keeps_settings_and_focus() {
        let st = NotifyState::default();
        assert!(!st.snapshot().0.paused);
        let s = NotificationSettings {
            paused: true,
            ..Default::default()
        };
        st.set_settings(s);
        st.set_focused_session(Some(sid("x")));
        let (settings, focused) = st.snapshot();
        assert!(settings.paused);
        assert_eq!(focused, Some(sid("x")));
    }
}
