import CoreText
import Foundation

/// Register bundled TTFs for Wawona Terminal (standalone and Wawona host).
/// Does not depend on WawonaApple `WWNBundleShareEnvironment`.
enum TerminalFontEnvironment {
  private static var fontsRegistered = false
  static let nerdFaceName = "DejaVuSansM Nerd Font Mono"

  static func registerIfNeeded() {
    guard !fontsRegistered else { return }
    fontsRegistered = true
    let fm = FileManager.default
    for root in shareRoots() {
      let fontDir = (root as NSString).appendingPathComponent("fonts")
      guard fm.fileExists(atPath: fontDir) else { continue }
      guard let enumerator = fm.enumerator(atPath: fontDir) else { continue }
      for case let rel as String in enumerator {
        let lower = rel.lowercased()
        guard lower.hasSuffix(".ttf") || lower.hasSuffix(".otf") else { continue }
        let path = (fontDir as NSString).appendingPathComponent(rel)
        let url = URL(fileURLWithPath: path) as CFURL
        CTFontManagerRegisterFontsForURL(url, .process, nil)
      }
    }
  }

  private static func shareRoots() -> [String] {
    var roots: [String] = []
    let main = Bundle.main.bundlePath
    roots.append((main as NSString).appendingPathComponent("share"))
    roots.append((main as NSString).appendingPathComponent("Contents/Resources/share"))
    if let res = Bundle.main.resourcePath {
      roots.append((res as NSString).appendingPathComponent("share"))
    }
    return roots
  }
}
