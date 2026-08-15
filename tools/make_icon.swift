// Render an emoji to a square PNG, for `tauri icon` to expand into the set.
//
// PIL and friends choke on Apple Color Emoji, so go through AppKit, which
// draws the real colour glyph.
//
//     swift tools/make_icon.swift 🔤 icon.png
import AppKit

let args = CommandLine.arguments
guard args.count > 2 else {
    FileHandle.standardError.write("usage: make_icon.swift <emoji> <out.png>\n".data(using: .utf8)!)
    exit(2)
}
let emoji = args[1]
let out = args[2]

let size: CGFloat = 1024
let image = NSImage(size: NSSize(width: size, height: size))

image.lockFocus()
// 0.8 leaves a little breathing room so macOS' rounded mask doesn't clip it.
guard let font = NSFont(name: "Apple Color Emoji", size: size * 0.8) else {
    FileHandle.standardError.write("Apple Color Emoji unavailable\n".data(using: .utf8)!)
    exit(1)
}
let str = NSAttributedString(string: emoji, attributes: [.font: font])
let bounds = str.boundingRect(
    with: NSSize(width: size, height: size),
    options: [.usesLineFragmentOrigin]
)
str.draw(at: NSPoint(x: (size - bounds.width) / 2, y: (size - bounds.height) / 2))
image.unlockFocus()

guard let tiff = image.tiffRepresentation,
      let rep = NSBitmapImageRep(data: tiff),
      let png = rep.representation(using: .png, properties: [:]) else {
    FileHandle.standardError.write("failed to encode PNG\n".data(using: .utf8)!)
    exit(1)
}
try png.write(to: URL(fileURLWithPath: out))
print("wrote \(out)")
