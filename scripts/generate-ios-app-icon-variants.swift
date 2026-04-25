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

func loadImage(at url: URL) throws -> CGImage {
  guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
        let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 1, userInfo: [
      NSLocalizedDescriptionKey: "Failed to load source icon at \(url.path)"
    ])
  }

  return image
}

func writePNG(_ image: CGImage, to url: URL) throws {
  guard let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 2, userInfo: [
      NSLocalizedDescriptionKey: "Failed to create output file at \(url.path)"
    ])
  }

  CGImageDestinationAddImage(destination, image, nil)
  guard CGImageDestinationFinalize(destination) else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 3, userInfo: [
      NSLocalizedDescriptionKey: "Failed to write PNG at \(url.path)"
    ])
  }
}

func aspectFitRect(sourceWidth: Int, sourceHeight: Int, targetSize: Int) -> CGRect {
  let target = CGFloat(targetSize)
  let sourceAspect = CGFloat(sourceWidth) / CGFloat(sourceHeight)
  let targetAspect: CGFloat = 1

  if sourceAspect > targetAspect {
    let height = target / sourceAspect
    return CGRect(x: 0, y: (target - height) / 2, width: target, height: height)
  }

  let width = target * sourceAspect
  return CGRect(x: (target - width) / 2, y: 0, width: width, height: target)
}

func renderIcon(source: CGImage, size: Int, background: HexColor?) throws -> CGImage {
  let bitmapInfo: UInt32
  if background == nil {
    bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
  } else {
    bitmapInfo = CGImageAlphaInfo.noneSkipLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
  }

  guard let context = CGContext(
    data: nil,
    width: size,
    height: size,
    bitsPerComponent: 8,
    bytesPerRow: size * 4,
    space: CGColorSpaceCreateDeviceRGB(),
    bitmapInfo: bitmapInfo
  ) else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 4, userInfo: [
      NSLocalizedDescriptionKey: "Failed to create drawing context"
    ])
  }

  let bounds = CGRect(x: 0, y: 0, width: size, height: size)
  if let background {
    context.setFillColor(CGColor(red: background.red, green: background.green, blue: background.blue, alpha: 1))
    context.fill(bounds)
  } else {
    context.clear(bounds)
  }

  context.interpolationQuality = .high
  context.draw(source, in: aspectFitRect(sourceWidth: source.width, sourceHeight: source.height, targetSize: size))

  guard let rendered = context.makeImage() else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 5, userInfo: [
      NSLocalizedDescriptionKey: "Failed to finalize icon image"
    ])
  }

  return rendered
}

func renderTintedIcon(source: CGImage, size: Int) throws -> CGImage {
  let bytesPerPixel = 4
  let bytesPerRow = size * bytesPerPixel
  let byteCount = bytesPerRow * size
  guard let data = calloc(byteCount, MemoryLayout<UInt8>.size) else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 6, userInfo: [
      NSLocalizedDescriptionKey: "Failed to allocate image buffer"
    ])
  }
  defer { free(data) }

  let bitmapInfo = CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
  guard let context = CGContext(
    data: data,
    width: size,
    height: size,
    bitsPerComponent: 8,
    bytesPerRow: bytesPerRow,
    space: CGColorSpaceCreateDeviceRGB(),
    bitmapInfo: bitmapInfo
  ) else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 7, userInfo: [
      NSLocalizedDescriptionKey: "Failed to create tinted drawing context"
    ])
  }

  context.clear(CGRect(x: 0, y: 0, width: size, height: size))
  context.interpolationQuality = .high
  context.draw(source, in: aspectFitRect(sourceWidth: source.width, sourceHeight: source.height, targetSize: size))

  let pixels = data.assumingMemoryBound(to: UInt8.self)
  for offset in stride(from: 0, to: byteCount, by: bytesPerPixel) {
    let alpha = Double(pixels[offset + 3]) / 255.0
    guard alpha > 0 else {
      continue
    }

    let red = min(255.0, Double(pixels[offset]) / alpha)
    let green = min(255.0, Double(pixels[offset + 1]) / alpha)
    let blue = min(255.0, Double(pixels[offset + 2]) / alpha)
    let luma = (0.299 * red) + (0.587 * green) + (0.114 * blue)
    let premultipliedLuma = UInt8(max(0, min(255, Int((luma * alpha).rounded()))))

    pixels[offset] = premultipliedLuma
    pixels[offset + 1] = premultipliedLuma
    pixels[offset + 2] = premultipliedLuma
  }

  guard let rendered = context.makeImage() else {
    throw NSError(domain: "generate-ios-app-icon-variants", code: 8, userInfo: [
      NSLocalizedDescriptionKey: "Failed to finalize tinted icon image"
    ])
  }

  return rendered
}

let arguments = CommandLine.arguments.dropFirst()
guard arguments.count >= 2 else {
  fputs("usage: generate-ios-app-icon-variants.swift <source_icon> <AppIcon.appiconset> [light_background_hex]\n", stderr)
  exit(64)
}

let sourceURL = URL(fileURLWithPath: arguments[arguments.startIndex])
let appIconSetURL = URL(fileURLWithPath: arguments[arguments.index(after: arguments.startIndex)], isDirectory: true)
let lightBackground = HexColor(arguments.dropFirst(2).first ?? "FFFAF2")
let iconSize = 1024
let source = try loadImage(at: sourceURL)

try FileManager.default.createDirectory(at: appIconSetURL, withIntermediateDirectories: true)

try writePNG(
  try renderIcon(source: source, size: iconSize, background: lightBackground),
  to: appIconSetURL.appendingPathComponent("AppIcon-Light.png")
)
try writePNG(
  try renderIcon(source: source, size: iconSize, background: nil),
  to: appIconSetURL.appendingPathComponent("AppIcon-Dark.png")
)
try writePNG(
  try renderTintedIcon(source: source, size: iconSize),
  to: appIconSetURL.appendingPathComponent("AppIcon-Tinted.png")
)
