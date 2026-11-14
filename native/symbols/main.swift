// lotse-symbols: render an SF Symbol to a template PNG (black glyph on transparent) for the webview.
//
//   lotse-symbols <symbol-name> <point-size> <weight> <out-dir>
//   lotse-symbols folder 16 regular ~/Library/Caches/Lotse/symbols   -> prints the PNG path
//
// Renders @1x and @2x (`<name>-<size>-<weight>.png`, `…@2x.png`); the app tints them with CSS
// `mask-image`, so one file serves light and dark. Existing files are reused (the cache).
// Exit codes: 0 ok, 1 unknown symbol or render failure, 2 usage. Local only: no network.
//
// Unverified: written without compiling (swiftc was not runnable in the WP7 delegated run).
// Check against https://developer.apple.com/documentation/appkit/nsimage/init(systemsymbolname:accessibilitydescription:)
// and https://developer.apple.com/documentation/appkit/nsimage/symbolconfiguration

import AppKit

let weights: [String: NSFont.Weight] = [
    "ultralight": .ultraLight, "thin": .thin, "light": .light, "regular": .regular,
    "medium": .medium, "semibold": .semibold, "bold": .bold, "heavy": .heavy, "black": .black,
]

func fail(_ message: String, code: Int32) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(code)
}

let args = CommandLine.arguments
guard args.count == 5, let size = Int(args[2]), size > 0, size <= 256, let weight = weights[args[3].lowercased()] else {
    fail("usage: lotse-symbols <symbol-name> <point-size 1…256> <weight> <out-dir>", code: 2)
}
let name = args[1]
// Symbol names are dotted lowercase words (`folder.badge.plus`); refuse anything else as a file name.
guard !name.isEmpty, name.allSatisfy({ $0.isLetter || $0.isNumber || $0 == "." || $0 == "-" || $0 == "_" }), !name.hasPrefix(".") else {
    fail("invalid symbol name: \(name)", code: 2)
}
let outDir = URL(fileURLWithPath: (args[4] as NSString).expandingTildeInPath, isDirectory: true)

func render(scale: Int) -> Data? {
    let config = NSImage.SymbolConfiguration(pointSize: CGFloat(size), weight: weight)
    // Unverified: `withSymbolConfiguration` returns the symbol at its natural size for `pointSize`.
    guard let symbol = NSImage(systemSymbolName: name, accessibilityDescription: nil)?.withSymbolConfiguration(config) else { return nil }
    let w = Int((symbol.size.width * CGFloat(scale)).rounded(.up))
    let h = Int((symbol.size.height * CGFloat(scale)).rounded(.up))
    guard w > 0, h > 0,
          let rep = NSBitmapImageRep(
              bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4,
              hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
          let ctx = NSGraphicsContext(bitmapImageRep: rep)
    else { return nil }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = ctx
    // Draw as a template: black glyph, alpha carries the shape.
    NSColor.black.set()
    let rect = NSRect(x: 0, y: 0, width: w, height: h)
    symbol.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1)
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])
}

do {
    try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
} catch {
    fail("cannot create \(outDir.path): \(error.localizedDescription)", code: 1)
}

let stem = "\(name)-\(size)-\(args[3].lowercased())"
for scale in [1, 2] {
    let file = outDir.appendingPathComponent(scale == 1 ? "\(stem).png" : "\(stem)@2x.png")
    if FileManager.default.fileExists(atPath: file.path) { continue }
    guard let png = render(scale: scale) else { fail("unknown symbol or render failed: \(name)", code: 1) }
    do { try png.write(to: file, options: .atomic) } catch { fail("write failed: \(error.localizedDescription)", code: 1) }
}
print(outDir.appendingPathComponent("\(stem).png").path)
