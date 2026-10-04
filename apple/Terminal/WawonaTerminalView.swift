import SwiftUI

#if os(iOS) || os(tvOS) || os(visionOS)
import UIKit
#endif

public struct WawonaTerminalSpan: Identifiable {
    public let id: Int
    public let text: String
    public let fg: UInt32
    public let bg: UInt32
    public let bold: Bool
    public let italic: Bool
    public let underline: Bool

    public init(
        id: Int, text: String, fg: UInt32, bg: UInt32, bold: Bool, italic: Bool, underline: Bool
    ) {
        self.id = id
        self.text = text
        self.fg = fg
        self.bg = bg
        self.bold = bold
        self.italic = italic
        self.underline = underline
    }
}

public struct WawonaTerminalLine: Identifiable {
    public let id: Int
    public let spans: [WawonaTerminalSpan]

    public init(id: Int, spans: [WawonaTerminalSpan]) {
        self.id = id
        self.spans = spans
    }
}

/// Styled lines for the SwiftUI scroll view. The VT screen stays in Rust.
@objc(WawonaTerminalModel)
public final class WawonaTerminalModel: NSObject, ObservableObject {
    @Published public var lines: [WawonaTerminalLine] = []

    @objc public override init() {
        super.init()
    }

    /// Little-endian pack from `wwn_term_export`: line count, then each line's
    /// cell count and cp, fg, bg, attrs.
    @objc public func applySnapshot(_ data: Data) {
        let next = WawonaTerminalSnapshot.lines(from: data)
        if Thread.isMainThread {
            lines = next
        } else {
            DispatchQueue.main.async { [weak self] in
                self?.lines = next
            }
        }
    }
}

enum WawonaTerminalSnapshot {
    static func lines(from data: Data) -> [WawonaTerminalLine] {
        if data.count < 4 {
            return []
        }
        var offset = 0
        let lineCount = readU32(data, &offset)
        var lines: [WawonaTerminalLine] = []
        lines.reserveCapacity(Int(lineCount))
        for lineIndex in 0..<Int(lineCount) {
            if offset + 4 > data.count {
                break
            }
            let cells = Int(readU32(data, &offset))
            var pieces: [(UInt32, UInt32, UInt32, UInt32)] = []
            pieces.reserveCapacity(cells)
            for _ in 0..<cells {
                if offset + 16 > data.count {
                    break
                }
                let cp = readU32(data, &offset)
                let fg = readU32(data, &offset)
                let bg = readU32(data, &offset)
                let attrs = readU32(data, &offset)
                pieces.append((cp, fg, bg, attrs))
            }
            var end = pieces.count
            while end > 0 {
                let cell = pieces[end - 1]
                if (cell.0 == 0 || cell.0 == 32) && cell.2 == 0 {
                    end -= 1
                } else {
                    break
                }
            }
            var spans: [WawonaTerminalSpan] = []
            var text = ""
            var style: (UInt32, UInt32, UInt32)?
            var spanID = 0
            for index in 0..<end {
                let cell = pieces[index]
                let key = (cell.1, cell.2, cell.3)
                if style != key {
                    if !text.isEmpty, let style {
                        spans.append(span(id: spanID, text: text, style: style))
                        spanID += 1
                        text = ""
                    }
                    style = key
                }
                let cp = cell.0 == 0 ? 32 : cell.0
                if let scalar = Unicode.Scalar(cp) {
                    text.append(Character(scalar))
                }
            }
            if !text.isEmpty, let style {
                spans.append(span(id: spanID, text: text, style: style))
            }
            lines.append(WawonaTerminalLine(id: lineIndex, spans: spans))
        }
        return lines
    }

    private static func span(id: Int, text: String, style: (UInt32, UInt32, UInt32))
        -> WawonaTerminalSpan
    {
        WawonaTerminalSpan(
            id: id,
            text: text,
            fg: style.0,
            bg: style.1,
            bold: style.2 & 1 != 0,
            italic: style.2 & 4 != 0,
            underline: style.2 & 8 != 0
        )
    }

    private static func readU32(_ data: Data, _ offset: inout Int) -> UInt32 {
        let b0 = UInt32(data[offset])
        let b1 = UInt32(data[offset + 1])
        let b2 = UInt32(data[offset + 2])
        let b3 = UInt32(data[offset + 3])
        offset += 4
        return b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }
}

public struct WawonaTerminalView: View {
    @ObservedObject var model: WawonaTerminalModel

    public init(model: WawonaTerminalModel) {
        self.model = model
    }

    public var body: some View {
        Group {
            if #available(iOS 14, macOS 11, tvOS 14, watchOS 7, visionOS 1, *) {
                pinned()
            } else {
                plain()
            }
        }
        .background(Color.black)
    }

    private func plain() -> some View {
        ScrollView {
            lineStack
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(8)
        }
    }

    @available(iOS 14, macOS 11, tvOS 14, watchOS 7, visionOS 1, *)
    private func pinned() -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                lineStack
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            .onChange(of: model.lines.count) { _ in
                if let last = model.lines.last {
                    proxy.scrollTo(last.id, anchor: .bottom)
                }
            }
        }
    }

    private var lineStack: some View {
        Group {
            if #available(iOS 14, macOS 11, tvOS 14, watchOS 7, visionOS 1, *) {
                LazyVStack(alignment: .leading, spacing: 0) {
                    rows
                }
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    rows
                }
            }
        }
    }

    private var rows: some View {
        ForEach(model.lines) { line in
            lineText(line)
                .frame(maxWidth: .infinity, alignment: .leading)
                .id(line.id)
        }
    }

    private func lineText(_ line: WawonaTerminalLine) -> Text {
        line.spans.reduce(Text("")) { acc, span in
            acc + spanText(span)
        }
        .font(.custom("DejaVuSansM Nerd Font Mono", size: 13))
    }

    private func spanText(_ span: WawonaTerminalSpan) -> Text {
        var text = Text(span.text).foregroundColor(rgb(span.fg))
        if span.bold {
            text = text.bold()
        }
        if span.italic {
            text = text.italic()
        }
        if span.underline {
            text = text.underline()
        }
        return text
    }

    private func rgb(_ value: UInt32) -> Color {
        Color(
            red: Double((value >> 16) & 0xFF) / 255,
            green: Double((value >> 8) & 0xFF) / 255,
            blue: Double(value & 0xFF) / 255
        )
    }
}

#if os(iOS) || os(tvOS) || os(visionOS)
@objc(WawonaTerminalViewFactory)
public final class WawonaTerminalViewFactory: NSObject {
    @objc public static func hostingController(model: WawonaTerminalModel) -> UIViewController {
        let host = UIHostingController(rootView: WawonaTerminalView(model: model))
        host.view.backgroundColor = .black
        return host
    }
}
#endif
