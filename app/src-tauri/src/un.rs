//! UNUserNotificationCenter: actionable notifications (Allow / Deny / Always allow on a permission
//! request). [`init`] runs once from the Tauri `setup`. It needs a bundled app: under `tauri dev` and in
//! tests [`is_active`] stays false and [`crate::notify`] falls back to the notification plugin.
//!
//! A tapped button becomes `ResolvePermission { via: notification, prompt_id }` through the
//! [`crate::daemon::Bridge`] (see [`crate::notify::action_request`]); a body click focuses the window
//! and tells the webview which session to select ([`OPEN_SESSION_EVENT`]).

use std::sync::atomic::{AtomicBool, Ordering};

/// Webview event raised by a click on a notification's body; the payload is the session id.
pub const OPEN_SESSION_EVENT: &str = "lotse://notification/open";

/// Set once the notification center is set up (bundled app only).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// What to play with a banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sound {
    Off,
    Default,
    /// A macOS system sound by name (`Glass`).
    Named(String),
}

/// One banner. `id` replaces a banner shown with the same id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Banner {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub sound: Sound,
    /// `Some(PERM_CATEGORY)`: Allow / Deny / Always allow buttons.
    pub category: Option<&'static str>,
    /// The prompt the buttons answer, carried in the banner itself.
    pub prompt_id: Option<String>,
}

/// Whether banners go through the notification center (and can carry buttons).
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Whether actionable notifications are wired up (false under `tauri dev`).
#[tauri::command]
pub fn un_available() -> bool {
    is_active()
}

#[cfg(target_os = "macos")]
pub use native::{init, remove, show};

#[cfg(not(target_os = "macos"))]
pub fn init(_app: &tauri::AppHandle) -> tauri::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn show(_banner: &Banner) -> Result<(), String> {
    Err("notification center is not available".into())
}

#[cfg(not(target_os = "macos"))]
pub fn remove(_id: &str) {}

#[cfg(target_os = "macos")]
mod native {
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, OnceLock};

    use block2::RcBlock;
    use lotse_proto::{Response, SessionId};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AnyThread, DefinedClass, define_class, msg_send};
    use objc2_foundation::{NSArray, NSBundle, NSDictionary, NSError, NSSet, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationAction,
        UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
        UNNotificationDefaultActionIdentifier, UNNotificationPresentationOptions,
        UNNotificationRequest, UNNotificationResponse, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use tauri::{AppHandle, Emitter, Manager, Wry};

    use super::{ACTIVE, Banner, OPEN_SESSION_EVENT, Sound};
    use crate::daemon::Bridge;
    use crate::notify::{
        ALLOW_ACTION, ALWAYS_ACTION, DENY_ACTION, PERM_CATEGORY, action_request, banner_id,
    };

    /// The key of the prompt id in a banner's `userInfo`.
    const PROMPT_KEY: &str = "prompt_id";

    struct Ivars {
        app: AppHandle<Wry>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the class has no Drop.
        #[unsafe(super(NSObject))]
        #[ivars = Ivars]
        #[name = "LotseNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            // The app is in front: show the banner anyway, the app already decided it is wanted.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &objc2_user_notifications::UNNotification,
                handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                handler: &block2::DynBlock<dyn Fn()>,
            ) {
                respond(&self.ivars().app, response);
                handler.call(());
            }
        }
    );

    // SAFETY: the only state is a Tauri `AppHandle`, which is Send + Sync; the class is immutable.
    unsafe impl Send for Delegate {}
    unsafe impl Sync for Delegate {}

    impl Delegate {
        fn new(app: AppHandle<Wry>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars { app });
            // SAFETY: NSObject's `init`.
            unsafe { msg_send![super(this), init] }
        }
    }

    /// The notification center keeps its delegate weakly.
    static DELEGATE: OnceLock<Retained<Delegate>> = OnceLock::new();

    fn ns(s: &str) -> Retained<NSString> {
        NSString::from_str(s)
    }

    /// `UNUserNotificationCenter` aborts in a process that is not an app bundle.
    fn bundled() -> bool {
        NSBundle::mainBundle()
            .bundlePath()
            .to_string()
            .ends_with(".app")
    }

    pub fn init(app: &AppHandle<Wry>) -> tauri::Result<()> {
        if !bundled() {
            tracing::debug!("un: not a bundled app, notifications stay with the plugin");
            return Ok(());
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = DELEGATE.get_or_init(|| Delegate::new(app.clone()));
        center.setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
        center.setNotificationCategories(&NSSet::from_retained_slice(&[perm_category()]));
        let done = RcBlock::new(|granted: objc2::runtime::Bool, error: *mut NSError| {
            if !granted.as_bool() {
                // SAFETY: the system passes a valid NSError or null.
                let why = unsafe { error.as_ref() }
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_default();
                tracing::warn!("un: notifications are not allowed for Lotse {why}");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &done,
        );
        ACTIVE.store(true, Ordering::Release);
        Ok(())
    }

    /// Allow, Deny (destructive) and Always allow; macOS shows the first as the button and the rest
    /// under Options.
    fn perm_category() -> Retained<UNNotificationCategory> {
        let action = |id, title, options| {
            UNNotificationAction::actionWithIdentifier_title_options(&ns(id), &ns(title), options)
        };
        let actions = NSArray::from_retained_slice(&[
            action(ALLOW_ACTION, "Allow", UNNotificationActionOptions::empty()),
            action(
                DENY_ACTION,
                "Deny",
                UNNotificationActionOptions::Destructive,
            ),
            action(
                ALWAYS_ACTION,
                "Always allow",
                UNNotificationActionOptions::empty(),
            ),
        ]);
        UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &ns(PERM_CATEGORY),
            &actions,
            &NSArray::new(),
            UNNotificationCategoryOptions::empty(),
        )
    }

    pub fn show(banner: &Banner) -> Result<(), String> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&ns(&banner.title));
        content.setSubtitle(&ns(&banner.subtitle));
        content.setBody(&ns(&banner.body));
        match &banner.sound {
            Sound::Off => {}
            Sound::Default => content.setSound(Some(&UNNotificationSound::defaultSound())),
            Sound::Named(name) => {
                content.setSound(Some(&UNNotificationSound::soundNamed(&ns(name))))
            }
        }
        if let Some(category) = banner.category {
            content.setCategoryIdentifier(&ns(category));
        }
        if let Some(prompt) = &banner.prompt_id {
            let info = NSDictionary::from_retained_objects(&[&*ns(PROMPT_KEY)], &[ns(prompt)]);
            // SAFETY: a dictionary of strings is a valid property list.
            unsafe { content.setUserInfo(info.cast_unchecked::<AnyObject, AnyObject>()) };
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &ns(&banner.id),
            &content,
            None,
        );
        let done = RcBlock::new(|error: *mut NSError| {
            // SAFETY: the system passes a valid NSError or null.
            if let Some(e) = unsafe { error.as_ref() } {
                tracing::warn!(
                    "un: showing the banner failed: {}",
                    e.localizedDescription()
                );
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&done));
        Ok(())
    }

    /// Take the banner with this id off the screen and out of the Notification Center.
    pub fn remove(id: &str) {
        if super::is_active() {
            UNUserNotificationCenter::currentNotificationCenter()
                .removeDeliveredNotificationsWithIdentifiers(&NSArray::from_retained_slice(&[ns(
                    id,
                )]));
        }
    }

    /// A button or the body was tapped.
    fn respond(app: &AppHandle<Wry>, response: &UNNotificationResponse) {
        let action = response.actionIdentifier().to_string();
        let content = response.notification().request().content();
        let session = SessionId::new(response.notification().request().identifier().to_string());
        let prompt = prompt_id(&content.userInfo());
        // SAFETY: a constant of the framework.
        let body_click = action == unsafe { UNNotificationDefaultActionIdentifier }.to_string();
        if body_click {
            open_session(app, &session);
        } else if let Some(request) = action_request(&session, prompt.as_deref(), &action) {
            let bridge = Arc::clone(&app.state::<Arc<Bridge>>());
            tauri::async_runtime::spawn(async move {
                match bridge.request(request).await {
                    Ok(Response::Ok) => {}
                    // `conflict`: the prompt is gone or answered; nothing was typed
                    Ok(other) => tracing::info!("un: the answer was refused: {other:?}"),
                    Err(e) => tracing::warn!("un: the answer was not sent: {e}"),
                }
            });
        }
        // an answered or stale banner has nothing left to offer
        if !body_click {
            remove(&banner_id(&session));
        }
    }

    fn prompt_id(info: &NSDictionary) -> Option<String> {
        let key = ns(PROMPT_KEY);
        let key: &AnyObject = &key;
        let value = info.objectForKey(key)?;
        Some(value.downcast::<NSString>().ok()?.to_string())
    }

    /// Bring the window forward and have the webview select the session.
    fn open_session(app: &AppHandle<Wry>, session: &SessionId) {
        if let Some(window) = app.webview_windows().into_values().next() {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        if let Err(e) = app.emit(OPEN_SESSION_EVENT, session.to_string()) {
            tracing::warn!("un: emitting {OPEN_SESSION_EVENT}: {e}");
        }
    }
}
