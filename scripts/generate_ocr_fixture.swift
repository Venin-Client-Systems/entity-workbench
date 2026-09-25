// Development fixture generation only. The application never invokes Swift or AppKit for OCR.
import AppKit
let width = 1200, height = 230
let pixels = UnsafeMutablePointer<UInt8>.allocate(capacity: width * height)
defer { pixels.deallocate() }
let context = CGContext(data: pixels, width: width, height: height, bitsPerComponent: 8,
                        bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(), bitmapInfo: 0)!
context.setFillColor(gray: 1, alpha: 1)
context.fill(CGRect(x: 0, y: 0, width: width, height: height))
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.monospacedSystemFont(ofSize: 42, weight: .regular), .foregroundColor: NSColor.black]
("SYNTHETIC OCR TEST" as NSString).draw(at: NSPoint(x: 30, y: 140), withAttributes: attributes)
("REFERENCE 0042 AMOUNT 123.45" as NSString).draw(at: NSPoint(x: 30, y: 70), withAttributes: attributes)
NSGraphicsContext.restoreGraphicsState()
var output = Data("P5\n\(width) \(height)\n255\n".utf8)
output.append(pixels, count: width * height)
try output.write(to: URL(fileURLWithPath: "fixtures/ocr/synthetic.pgm"))
