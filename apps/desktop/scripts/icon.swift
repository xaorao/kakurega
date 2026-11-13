import AppKit
// usage: swift apps/desktop/scripts/icon.swift OUT.iconset && iconutil -c icns OUT.iconset -o apps/desktop/assets/kakurega.icns
// The web app's mark (apps/web/public/favicon.svg) at macOS icon sizes: a near-black tile on the standard icon grid
// (the tile 824 of 1024 with a margin, corners about 22%), three bars, the last in the green kept for proof.
let out = CommandLine.arguments[1]
let fm = FileManager.default
try? fm.removeItem(atPath: out)
try! fm.createDirectory(atPath: out, withIntermediateDirectories: true)
func draw(_ px: Int) -> Data {
    let s = CGFloat(px)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let tile = s * 824 / 1024, m = (s - tile) / 2
    let r = NSRect(x: m, y: m, width: tile, height: tile)
    NSColor(srgbRed: 0x0A/255, green: 0x0A/255, blue: 0x0A/255, alpha: 1).setFill()
    NSBezierPath(roundedRect: r, xRadius: tile * 0.2237, yRadius: tile * 0.2237).fill()
    // the favicon's bars on its 32-unit grid, mapped into the tile (y measured from the top there)
    let u = tile / 32
    func bar(_ x: CGFloat, _ y: CGFloat, _ w: CGFloat, _ c: NSColor) {
        c.setFill()
        NSRect(x: m + x * u, y: m + tile - (y + 3) * u, width: w * u, height: 3 * u).fill()
    }
    let ink = NSColor(srgbRed: 0xFA/255, green: 0xFA/255, blue: 0xF7/255, alpha: 1)
    bar(7, 9, 18, ink)
    bar(7, 15, 12, ink)
    bar(7, 21, 18, NSColor(srgbRed: 0x05/255, green: 0x96/255, blue: 0x69/255, alpha: 1))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}
for (name, px) in [("icon_16x16", 16), ("icon_16x16@2x", 32), ("icon_32x32", 32), ("icon_32x32@2x", 64), ("icon_128x128", 128), ("icon_128x128@2x", 256), ("icon_256x256", 256), ("icon_256x256@2x", 512), ("icon_512x512", 512), ("icon_512x512@2x", 1024)] {
    try! draw(px).write(to: URL(fileURLWithPath: "\(out)/\(name).png"))
}
