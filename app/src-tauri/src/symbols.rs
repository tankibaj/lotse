//! SF Symbols for project icons, rendered at runtime with `NSImage(systemSymbolName:)`.
//!
//! `symbol_png(name, point_size, scale)` returns a base64 PNG: the symbol drawn black on a transparent
//! square canvas (a template image), so the UI tints it with the project colour by using the PNG as
//! a CSS mask. Nothing is exported to disk; the glyphs exist only in memory at runtime, which is what
//! Apple's licence allows for an app on its platforms. Results are cached per name, size and scale.
//! AppKit drawing belongs to the main thread, so the command hops there on a cache miss.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Symbol names are lowercase dotted words (`folder.fill`, `arrow.triangle.branch`).
const MAX_NAME: usize = 64;
const MAX_CACHE: usize = 1024;

type Key = (String, u32, u32);

static CACHE: LazyLock<Mutex<HashMap<Key, String>>> = LazyLock::new(Mutex::default);

/// A cache key for a valid request, or the reason it is rejected.
fn key(name: &str, point_size: f64, scale: f64) -> Result<Key, String> {
    let valid_name = !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.');
    if !valid_name {
        return Err(format!("not an SF Symbol name: {name:?}"));
    }
    if !(4.0..=256.0).contains(&point_size) {
        return Err("point_size must be 4 to 256".into());
    }
    if !(1.0..=4.0).contains(&scale) {
        return Err("scale must be 1 to 4".into());
    }
    // quarter points and half scales are all the UI asks for
    Ok((
        name.to_string(),
        (point_size * 4.0).round() as u32,
        (scale * 2.0).round() as u32,
    ))
}

fn cached(key: &Key) -> Option<String> {
    CACHE.lock().ok()?.get(key).cloned()
}

fn remember(key: Key, png: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() >= MAX_CACHE {
            cache.clear();
        }
        cache.insert(key, png.to_string());
    }
}

/// Standard base64 with padding (the one use is a PNG for a data URL; no crate needed for that).
fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
        NSGraphicsContext, NSImage, NSImageSymbolConfiguration, NSImageSymbolScale,
    };
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};

    /// The symbol drawn black into a square, transparent RGBA bitmap `side` pixels wide.
    pub fn draw(
        name: &str,
        point_size: f64,
        scale: f64,
    ) -> Result<Retained<NSBitmapImageRep>, String> {
        let symbol = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(name),
            None,
        )
        .ok_or_else(|| format!("unknown SF Symbol: {name}"))?;
        let config = NSImageSymbolConfiguration::configurationWithPointSize_weight_scale(
            point_size,
            0.0, // NSFontWeightRegular
            NSImageSymbolScale::Medium,
        );
        let symbol = symbol
            .imageWithSymbolConfiguration(&config)
            .ok_or_else(|| format!("cannot configure SF Symbol: {name}"))?;
        symbol.setTemplate(true);

        let natural = symbol.size();
        let longest = natural.width.max(natural.height);
        if longest <= 0.0 {
            return Err(format!("empty SF Symbol: {name}"));
        }
        let side = (longest * scale).ceil();
        let (w, h) = (natural.width * scale, natural.height * scale);

        // SAFETY: null planes ask AppKit to allocate the pixel buffer itself.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                side as isize,
                side as isize,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }
        .ok_or("cannot allocate a bitmap")?;
        let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)
            .ok_or("cannot draw into a bitmap")?;

        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&ctx));
        symbol.drawInRect_fromRect_operation_fraction(
            NSRect::new(
                NSPoint::new((side - w) / 2.0, (side - h) / 2.0),
                NSSize::new(w, h),
            ),
            NSRect::ZERO, // the whole image
            NSCompositingOperation::SourceOver,
            1.0,
        );
        ctx.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        Ok(rep)
    }

    pub fn png(rep: &NSBitmapImageRep) -> Result<Vec<u8>, String> {
        let props = NSDictionary::<NSString, AnyObject>::new();
        // SAFETY: an empty properties dictionary is valid for PNG.
        unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }
            .map(|data| data.to_vec())
            .ok_or_else(|| "cannot encode the PNG".into())
    }
}

/// Render now, on the calling thread (the main thread in the app).
#[cfg(target_os = "macos")]
fn render(name: &str, point_size: f64, scale: f64) -> Result<Vec<u8>, String> {
    let rep = mac::draw(name, point_size, scale)?;
    mac::png(&rep)
}

#[cfg(not(target_os = "macos"))]
fn render(_: &str, _: f64, _: f64) -> Result<Vec<u8>, String> {
    Err("SF Symbols are available on macOS only".into())
}

/// A base64 PNG of the SF Symbol `name`: black on transparent, a `point_size` symbol drawn at
/// `scale` (2 on a Retina display) on a square canvas. Must run on the main thread.
pub fn symbol_png_base64(name: &str, point_size: f64, scale: f64) -> Result<String, String> {
    let key = key(name, point_size, scale)?;
    if let Some(hit) = cached(&key) {
        return Ok(hit);
    }
    let png = base64(&render(name, point_size, scale)?);
    remember(key, &png);
    Ok(png)
}

/// `symbol_png(name, point_size, scale)`: a base64 PNG for the UI to use as a CSS mask. An unknown
/// name is an error (the UI then falls back to its Lucide icon).
#[tauri::command]
pub async fn symbol_png(
    app: tauri::AppHandle,
    name: String,
    point_size: f64,
    scale: f64,
) -> Result<String, String> {
    let key = key(&name, point_size, scale)?;
    if let Some(hit) = cached(&key) {
        return Ok(hit);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(symbol_png_base64(&name, point_size, scale));
    })
    .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string())?)
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (raw, enc) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(raw.as_bytes()), enc);
        }
    }

    #[test]
    fn rejects_names_that_are_not_symbol_names_and_out_of_range_sizes() {
        for bad in [
            "",
            "Folder",
            "a b",
            "../x",
            ".hidden",
            "a..b",
            "x.",
            &"a".repeat(65),
        ] {
            assert!(key(bad, 16.0, 2.0).is_err(), "{bad:?}");
        }
        assert!(key("folder", 2.0, 2.0).is_err());
        assert!(key("folder", 16.0, 8.0).is_err());
        assert_eq!(
            key("folder.fill", 16.0, 2.0),
            Ok(("folder.fill".into(), 64, 4))
        );
    }

    #[cfg(target_os = "macos")]
    mod native {
        use super::*;

        /// Decode nothing: the PNG header says how big and what colour type the image is.
        fn header(png: &[u8]) -> (u32, u32, u8) {
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");
            assert_eq!(&png[12..16], b"IHDR");
            let be = |i: usize| u32::from_be_bytes(png[i..i + 4].try_into().unwrap());
            (be(16), be(20), png[25])
        }

        fn unbase64(s: &str) -> Vec<u8> {
            let val = |c: u8| match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                _ => 63,
            };
            let digits: Vec<u8> = s.bytes().filter(|&c| c != b'=').map(val).collect();
            digits
                .chunks(4)
                .flat_map(|c| {
                    let n =
                        c.iter().fold(0u32, |n, d| n << 6 | u32::from(*d)) << (6 * (4 - c.len()));
                    (0..c.len() - 1).map(move |i| (n >> (16 - 8 * i)) as u8)
                })
                .collect()
        }

        #[test]
        fn folder_renders_a_square_rgba_png() {
            // `cargo test` runs on a worker thread; drawing into a bitmap works there, and the command
            // itself always runs on the main thread (same approach as the fonts test).
            let png = unbase64(&symbol_png_base64("folder", 16.0, 2.0).expect("folder renders"));
            let (w, h, colour_type) = header(&png);
            assert_eq!(w, h, "square canvas");
            assert!(w >= 16, "at least the point size at scale 2: {w}");
            assert_eq!(colour_type, 6, "RGBA");
        }

        #[test]
        fn the_symbol_is_drawn_not_blank_and_transparent_around_it() {
            let rep = mac::draw("folder.fill", 32.0, 2.0).expect("folder.fill draws");
            let (w, h) = (rep.pixelsWide() as usize, rep.pixelsHigh() as usize);
            let stride = rep.bytesPerRow() as usize;
            // SAFETY: bitmapData points at `stride * h` bytes owned by `rep`, alive for this function.
            let pixels = unsafe { std::slice::from_raw_parts(rep.bitmapData(), stride * h) };
            let alpha = |x: usize, y: usize| pixels[y * stride + x * 4 + 3];
            let opaque = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .filter(|&(x, y)| alpha(x, y) > 200)
                .count();
            assert!(opaque > w * h / 8, "{opaque} opaque pixels of {}", w * h);
            assert_eq!(alpha(0, 0), 0, "corner is transparent");
        }

        #[test]
        fn a_bogus_name_is_an_error_and_is_not_cached() {
            let err = symbol_png_base64("not.a.real.symbol.name", 16.0, 2.0).unwrap_err();
            assert!(err.contains("unknown SF Symbol"), "{err}");
            let key = key("not.a.real.symbol.name", 16.0, 2.0).unwrap();
            assert!(cached(&key).is_none());
        }

        #[test]
        fn the_second_request_is_served_from_the_cache() {
            let first = symbol_png_base64("star.fill", 20.0, 2.0).unwrap();
            let key = key("star.fill", 20.0, 2.0).unwrap();
            assert_eq!(cached(&key).as_deref(), Some(first.as_str()));
            assert_eq!(symbol_png_base64("star.fill", 20.0, 2.0).unwrap(), first);
        }

        /// The picker's catalogue (`app/src/sheets/symbols.json`) must only name symbols this macOS has.
        /// A name missing here falls back to its Lucide icon in the app, but that is never intended.
        #[test]
        fn every_catalogue_symbol_renders() {
            #[derive(serde::Deserialize)]
            struct Entry {
                id: String,
                sf: String,
            }
            let entries: Vec<Entry> =
                serde_json::from_str(include_str!("../../src/sheets/symbols.json")).unwrap();
            assert!(entries.len() >= 100, "{} entries", entries.len());
            let missing: Vec<String> = entries
                .iter()
                .filter(|e| mac::draw(&e.sf, 16.0, 2.0).is_err())
                .map(|e| format!("{} ({})", e.id, e.sf))
                .collect();
            assert!(missing.is_empty(), "no SF Symbol for: {missing:?}");
        }
    }
}
