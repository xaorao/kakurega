import AppKit
import Foundation
import ImageIO
import PDFKit
import Vision

// kaku-ocr --kind picture|pdf [--pages 1,2,3]
// Bytes on standard input, one line of JSON on standard output. Recognition runs on this computer through the
// operating system's own Vision framework; nothing is sent anywhere.

let maxInput = 12 * 1024 * 1024
let maxPixels = 50_000_000
let maxPdfPages = 20
let longestSide: CGFloat = 2600
let maxScale: CGFloat = 3.0

func answer(_ object: [String: Any]) -> Never {
    let data = (try? JSONSerialization.data(withJSONObject: object, options: [])) ?? Data("{\"error\":\"unreadable\"}".utf8)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
    exit(0)
}

func fail(_ code: String) -> Never {
    answer(["error": code])
}

func argument(_ name: String) -> String? {
    let a = CommandLine.arguments
    guard let i = a.firstIndex(of: name), i + 1 < a.count else { return nil }
    return a[i + 1]
}

func recognise(_ image: CGImage) -> (text: String, confidence: Double)? {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true
    do {
        try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
    } catch {
        return nil
    }
    let found = (request.results ?? []).compactMap { o -> (VNRecognizedTextObservation, VNRecognizedText)? in
        guard let top = o.topCandidates(1).first else { return nil }
        return (o, top)
    }
    // reading order: top to bottom, then left to right; lines whose tops are close count as one row
    let rows = found.sorted { a, b in
        let ay = a.0.boundingBox.maxY, by = b.0.boundingBox.maxY
        if abs(ay - by) > 0.01 { return ay > by }
        return a.0.boundingBox.minX < b.0.boundingBox.minX
    }
    let text = rows.map { $0.1.string }.joined(separator: "\n")
    let mean = rows.isEmpty ? 0.0 : rows.map { Double($0.1.confidence) }.reduce(0, +) / Double(rows.count)
    return (text, mean)
}

var input = Data()
while input.count <= maxInput {
    let chunk = FileHandle.standardInput.readData(ofLength: 1 << 20)
    if chunk.isEmpty { break }
    input.append(chunk)
}
if input.isEmpty { fail("unreadable") }
if input.count > maxInput { fail("too_large") }

switch argument("--kind") {
case "picture":
    guard let source = CGImageSourceCreateWithData(input as CFData, nil), CGImageSourceGetCount(source) > 0 else { fail("unreadable") }
    guard let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
          let w = props[kCGImagePropertyPixelWidth] as? Int, let h = props[kCGImagePropertyPixelHeight] as? Int else { fail("unreadable") }
    if w <= 0 || h <= 0 || w * h > maxPixels { fail("too_large") }
    guard let image = CGImageSourceCreateImageAtIndex(source, 0, nil), let r = recognise(image) else { fail("unreadable") }
    answer(["pages": [["page": 1, "text": r.text, "confidence": r.confidence]], "skipped": []])
case "pdf":
    guard let doc = PDFDocument(data: input), !doc.isLocked else { fail("unreadable") }
    let wanted: [Int] = (argument("--pages") ?? "").split(separator: ",").compactMap { Int($0) }.filter { $0 >= 1 && $0 <= doc.pageCount }
    let chosen = wanted.isEmpty ? Array(1...max(1, doc.pageCount)).filter { $0 <= doc.pageCount } : wanted
    var pages: [[String: Any]] = []
    var skipped: [Int] = []
    for (i, n) in chosen.enumerated() {
        if i >= maxPdfPages { skipped.append(n); continue }
        guard let page = doc.page(at: n - 1) else { skipped.append(n); continue }
        let box = page.bounds(for: .mediaBox)
        let side = max(box.width, box.height)
        if side <= 0 || box.width * box.height > 4_000_000 { skipped.append(n); continue }
        let scale = min(maxScale, longestSide / side)
        let size = CGSize(width: ceil(box.width * scale), height: ceil(box.height * scale))
        guard let ctx = CGContext(data: nil, width: Int(size.width), height: Int(size.height), bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { skipped.append(n); continue }
        ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
        ctx.fill(CGRect(origin: .zero, size: size))
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: -box.minX, y: -box.minY)
        page.draw(with: .mediaBox, to: ctx)
        guard let image = ctx.makeImage(), let r = recognise(image) else { skipped.append(n); continue }
        pages.append(["page": n, "text": r.text, "confidence": r.confidence])
    }
    answer(["pages": pages, "skipped": skipped])
default:
    fail("unreadable")
}
