//! `src-tauri/Info.plist` is merged into the bundle's Info.plist by Tauri; the keys macOS needs
//! for the microphone and Dictation prompts and for actionable notifications must be there.

use std::path::PathBuf;

fn plist() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `<string>` right after `<key>name</key>`.
fn string_of(plist: &str, key: &str) -> Option<String> {
    let rest = plist.split(&format!("<key>{key}</key>")).nth(1)?;
    let rest = rest.trim_start().strip_prefix("<string>")?;
    Some(rest.split("</string>").next()?.to_owned())
}

#[test]
fn declares_the_usage_descriptions_and_the_alert_style() {
    let plist = plist();
    for key in [
        "NSMicrophoneUsageDescription",
        "NSSpeechRecognitionUsageDescription",
    ] {
        let text = string_of(&plist, key).unwrap_or_else(|| panic!("missing {key}"));
        assert!(!text.trim().is_empty(), "{key} is empty");
    }
    assert_eq!(
        string_of(&plist, "NSUserNotificationAlertStyle").as_deref(),
        Some("alert")
    );
}
