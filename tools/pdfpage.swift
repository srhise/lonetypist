import PDFKit
import AppKit
let a = CommandLine.arguments
let doc = PDFDocument(url: URL(fileURLWithPath: a[1]))!
let page = doc.page(at: Int(a[2])!)!
let scale = CGFloat(Double(a[4])!)
let box = page.bounds(for: .mediaBox)
// optional crop: x y w h in points from top-left
var crop = box
if a.count > 8 { crop = CGRect(x: Double(a[5])!, y: box.height - Double(a[6])! - Double(a[8])!, width: Double(a[7])!, height: Double(a[8])!) }
let w = Int(crop.width*scale), h = Int(crop.height*scale)
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
let ctx = NSGraphicsContext(bitmapImageRep: rep)!
let cg = ctx.cgContext
cg.setFillColor(NSColor.white.cgColor); cg.fill(CGRect(x:0,y:0,width:w,height:h))
cg.scaleBy(x: scale, y: scale); cg.translateBy(x: -crop.minX, y: -crop.minY)
page.draw(with: .mediaBox, to: cg)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[3]))
