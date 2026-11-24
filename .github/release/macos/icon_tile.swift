// Put Red Alert 2's red star (../icon.png) on a dark tile that fills macOS's
// icon grid, 824 px with 185 px corners on a 1024 px canvas, so macOS shows it
// as a standard app icon instead of on its grey stand-in tile (app.sh):
//   swift icon_tile.swift <star.png> <out.png>
// CoreGraphics only, so it draws without a window server.
import CoreGraphics
import Foundation
import ImageIO

let args = CommandLine.arguments
guard args.count == 3 else {
  FileHandle.standardError.write(Data("usage: icon_tile.swift <star.png> <out.png>\n".utf8))
  exit(1)
}
guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, nil),
  let star = CGImageSourceCreateImageAtIndex(source, 0, nil)
else {
  FileHandle.standardError.write(Data("icon_tile.swift: cannot read \(args[1])\n".utf8))
  exit(1)
}
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let context = CGContext(
  data: nil, width: 1024, height: 1024, bitsPerComponent: 8, bytesPerRow: 0, space: space,
  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
context.addPath(CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil))
context.clip()
// Dark red, lighter at the top (the y axis points up).
let shade = CGGradient(
  colorsSpace: space,
  colors: [
    CGColor(srgbRed: 0.06, green: 0.02, blue: 0.02, alpha: 1),
    CGColor(srgbRed: 0.24, green: 0.05, blue: 0.04, alpha: 1),
  ] as CFArray, locations: [0, 1])!
context.drawLinearGradient(
  shade, start: CGPoint(x: 512, y: tile.minY), end: CGPoint(x: 512, y: tile.maxY), options: [])
// A soft orange glow behind the star, fading out beyond the tile's edge.
let glow = CGGradient(
  colorsSpace: space,
  colors: [
    CGColor(srgbRed: 1, green: 0.35, blue: 0.1, alpha: 0.32),
    CGColor(srgbRed: 1, green: 0.35, blue: 0.1, alpha: 0),
  ] as CFArray, locations: [0, 1])!
context.drawRadialGradient(
  glow, startCenter: CGPoint(x: 512, y: 512), startRadius: 0,
  endCenter: CGPoint(x: 512, y: 512), endRadius: 472, options: [])
context.draw(star, in: tile.insetBy(dx: 64, dy: 64))
let output = CGImageDestinationCreateWithURL(
  URL(fileURLWithPath: args[2]) as CFURL, "public.png" as CFString, 1, nil)!
CGImageDestinationAddImage(output, context.makeImage()!, nil)
guard CGImageDestinationFinalize(output) else { exit(1) }
