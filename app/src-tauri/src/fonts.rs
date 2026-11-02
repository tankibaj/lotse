//! Installed font families for Settings › Appearance, read from macOS `NSFontManager`.
//!
//! `monospace` is the subset of `families` whose first face is fixed-pitch (that is how Nerd Fonts
//! and other programming fonts show up). Names starting with "." are system-private and hidden.
//! `NSFontManager` belongs to the main thread, so the command hops there.

use serde::Serialize;

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct Fonts {
    /// Every installed family, sorted case-insensitively, without duplicates.
    pub families: Vec<String>,
    /// The fixed-pitch families, same order.
    pub monospace: Vec<String>,
}

/// Drop blank and hidden (".SF NS", …) names, sort case-insensitively, remove duplicates.
pub fn normalize(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = names
        .into_iter()
        .filter(|n| !n.trim().is_empty() && !n.starts_with('.'))
        .collect();
    names.sort_by_key(|n| (n.to_lowercase(), n.clone()));
    // font-family matching is case-insensitive, so "Menlo" and "menlo" are one family
    names.dedup_by_key(|n| n.to_lowercase());
    names
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{Fonts, normalize};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSFontManager, NSFontTraitMask};
    use objc2_foundation::NSString;

    /// A family is monospace when its first face (PostScript name) is fixed-pitch.
    fn is_fixed_pitch(manager: &NSFontManager, family: &NSString) -> bool {
        let Some(members) = manager.availableMembersOfFontFamily(family) else {
            return false;
        };
        // each member is `[postscript name, style name, weight, traits]`
        let Some(first) = members.to_vec().into_iter().next() else {
            return false;
        };
        first
            .to_vec()
            .into_iter()
            .next()
            .and_then(|name| name.downcast::<NSString>().ok())
            .is_some_and(|name| {
                manager.fontNamed_hasTraits(&name, NSFontTraitMask::FixedPitchFontMask)
            })
    }

    pub fn collect(mtm: MainThreadMarker) -> Fonts {
        let manager = NSFontManager::sharedFontManager(mtm);
        let mut families = Vec::new();
        let mut monospace = Vec::new();
        for family in manager.availableFontFamilies().to_vec() {
            let name = family.to_string();
            if name.starts_with('.') {
                continue;
            }
            if is_fixed_pitch(&manager, &family) {
                monospace.push(name.clone());
            }
            families.push(name);
        }
        Fonts {
            families: normalize(families),
            monospace: normalize(monospace),
        }
    }
}

/// Must run on the main thread; elsewhere (or off macOS) the lists are empty.
fn installed() -> Fonts {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        return mac::collect(mtm);
    }
    Fonts::default()
}

/// `{ families, monospace }` of the fonts installed on this Mac.
#[tauri::command]
pub async fn list_fonts(app: tauri::AppHandle) -> Result<Fonts, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(installed());
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalize_sorts_case_insensitively_dedupes_and_hides_private_fonts() {
        let input = names(&["menlo", ".SF NS", "Zed", "", "arial", "Menlo", "Arial"]);
        assert_eq!(normalize(input), names(&["Arial", "Menlo", "Zed"]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn installed_fonts_are_sorted_deduped_and_include_a_monospace_family() {
        // `cargo test` runs on a worker thread. Reading the font list there is fine in practice;
        // the command itself always runs on the main thread.
        // SAFETY: only reads font names; nothing here relies on main-thread-only state.
        let mtm = unsafe { objc2::MainThreadMarker::new_unchecked() };
        let fonts = mac::collect(mtm);
        assert!(!fonts.families.is_empty());
        assert_eq!(fonts.families, normalize(fonts.families.clone()));
        assert_eq!(fonts.monospace, normalize(fonts.monospace.clone()));
        assert!(fonts.families.iter().all(|f| !f.starts_with('.')));
        let mono = &fonts.monospace;
        assert!(!mono.is_empty());
        assert!(mono.iter().all(|m| fonts.families.contains(m)));
        // every Mac ships at least one of these
        let known = ["Menlo", "Monaco", "Courier"];
        assert!(mono.iter().any(|m| known.contains(&m.as_str())), "{mono:?}");
    }
}
