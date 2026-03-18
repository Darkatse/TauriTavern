#!/usr/bin/env swift

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

struct HexColor {
  let red: CGFloat
  let green: CGFloat
  let blue: CGFloat

  init(_ hex: String) {
    let sanitized = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
    guard sanitized.count == 6, let value = UInt64(sanitized, radix: 16) else {
      fatalError("Invalid hex color: \(hex)")
    }

    red = CGFloat((value >> 16) & 0xFF) / 255.0
    green = CGFloat((value >> 8) & 0xFF) / 255.0
    blue = CGFloat(value & 0xFF) / 255.0
  }
}

func flattenPNG(at url: URL, background: HexColor) throws {
  guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
        let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
    throw NSError(domain: "ios-opaque-app-icons", code: 1, userInfo: [
      NSLocalizedDescriptionKey: "Failed to load image at \(url.path)"
    ])
  }

  switch image.alphaInfo {
  case .alphaOnly, .first, .last, .premultipliedFirst, .premultipliedLast:
    break
  default:
    return
  }

  let bitmapInfo = CGImageAlphaInfo.noneSkipLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
  guard let context = CGContext(
    data: nil,
    width: image.width,
    height: image.height,
    bitsPerComponent: 8,
    bytesPerRow: 0,
    space: CGColorSpaceCreateDeviceRGB(),
    bitmapInfo: bitmapInfo
  ) else {
    throw NSError(domain: "ios-opaque-app-icons", code: 2, userInfo: [
      NSLocalizedDescriptionKey: "Failed to create drawing context for \(url.path)"
    ])
  }

  context.setFillColor(CGColor(red: background.red, green: background.green, blue: background.blue, alpha: 1))
  context.fill(CGRect(x: 0, y: 0, width: image.width, height: image.height))
  context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))

  guard let flattened = context.makeImage() else {
    throw NSError(domain: "ios-opaque-app-icons", code: 3, userInfo: [
      NSLocalizedDescriptionKey: "Failed to finalize image for \(url.path)"
    ])
  }

  let tempURL = url.deletingLastPathComponent().appendingPathComponent(".\(url.lastPathComponent).tmp")
  guard let destination = CGImageDestinationCreateWithURL(tempURL as CFURL, UTType.png.identifier as CFString, 1, nil) else {
    throw NSError(domain: "ios-opaque-app-icons", code: 4, userInfo: [
      NSLocalizedDescriptionKey: "Failed to create output file for \(url.path)"
    ])
  }

  CGImageDestinationAddImage(destination, flattened, nil)
  guard CGImageDestinationFinalize(destination) else {
    throw NSError(domain: "ios-opaque-app-icons", code: 5, userInfo: [
      NSLocalizedDescriptionKey: "Failed to write PNG data for \(url.path)"
    ])
  }

  _ = try FileManager.default.replaceItemAt(url, withItemAt: tempURL)
}

let arguments = CommandLine.arguments.dropFirst()
guard let appIconSetArgument = arguments.first else {
  fputs("usage: ios-opaque-app-icons.swift <AppIcon.appiconset> [background_hex]\n", stderr)
  exit(64)
}

let background = HexColor(arguments.dropFirst().first ?? "FFF3D6")
let appIconSetURL = URL(fileURLWithPath: appIconSetArgument, isDirectory: true)
let fileManager = FileManager.default

guard fileManager.fileExists(atPath: appIconSetURL.path) else {
  exit(0)
}

let pngURLs = try fileManager.contentsOfDirectory(
  at: appIconSetURL,
  includingPropertiesForKeys: nil,
  options: [.skipsHiddenFiles]
)
  .filter { $0.pathExtension.lowercased() == "png" }

for pngURL in pngURLs {
  try flattenPNG(at: pngURL, background: background)
}
