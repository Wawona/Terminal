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

/// Styled lines for the SwiftUI scroll view.
/// The document is UniFFI `TerminalSession.lines()` (same JSON as `lines_json`).
@objc(WawonaTerminalModel)
public final class WawonaTerminalModel: NSObject, ObservableObject {
    @Published public var lines: [WawonaTerminalLine] = []
    private var nextLineID = 1
    private var lineIDs: [Int] = []
    private var lineKeys: [String] = []

    @objc public override init() {
        super.init()
    }

    @objc public func applyLines(_ document: String) {
        let parsed = WawonaTerminalDocument.lines(from: document)
        let keys = parsed.map { $0.spans.map(\.text).joined(separator: "\u{1}") }
        lineIDs = alignTerminalLineIDs(oldIDs: lineIDs, oldKeys: lineKeys, newKeys: keys) {
            let id = self.nextLineID
            self.nextLineID += 1
            return id
        }
        lineKeys = keys
        let next = zip(lineIDs, parsed).map { id, line in
            WawonaTerminalLine(
                id: id,
                spans: line.spans.enumerated().map { index, span in
                    WawonaTerminalSpan(
                        id: index,
                        text: span.text,
                        fg: span.fg,
                        bg: span.bg,
                        bold: span.bold,
                        italic: span.italic,
                        underline: span.underline
                    )
                }
            )
        }
        if Thread.isMainThread {
            lines = next
        } else {
            DispatchQueue.main.async { [weak self] in
                self?.lines = next
            }
        }
    }
}

private func alignTerminalLineIDs(
    oldIDs: [Int],
    oldKeys: [String],
    newKeys: [String],
    mint: () -> Int
) -> [Int] {
    if newKeys == oldKeys, newKeys.count == oldIDs.count {
        return oldIDs
    }
    if newKeys.count >= oldKeys.count, Array(newKeys.prefix(oldKeys.count)) == oldKeys {
        var ids = oldIDs
        while ids.count < newKeys.count {
            ids.append(mint())
        }
        return ids
    }
    if !oldKeys.isEmpty, newKeys.count == oldKeys.count,
        Array(newKeys.dropLast()) == Array(oldKeys.dropFirst())
    {
        var ids = Array(oldIDs.dropFirst())
        ids.append(mint())
        return ids
    }
    var ids: [Int] = []
    let overlap = min(oldIDs.count, newKeys.count)
    for _ in 0..<(newKeys.count - overlap) {
        ids.append(mint())
    }
    ids.append(contentsOf: oldIDs.suffix(overlap))
    return ids
}

private struct TermLineRecord: Decodable {
    struct Span: Decodable {
        let text: String
        let fg: UInt32
        let bg: UInt32
        let bold: Bool
        let italic: Bool
        let underline: Bool
    }

    let spans: [Span]
}

enum WawonaTerminalDocument {
    static func lines(from document: String) -> [WawonaTerminalLine] {
        let data = Data(document.utf8)
        let decoded = (try? JSONDecoder().decode([TermLineRecord].self, from: data)) ?? []
        return decoded.enumerated().map { index, line in
            WawonaTerminalLine(
                id: index,
                spans: line.spans.enumerated().map { spanIndex, span in
                    WawonaTerminalSpan(
                        id: spanIndex,
                        text: span.text,
                        fg: span.fg,
                        bg: span.bg,
                        bold: span.bold,
                        italic: span.italic,
                        underline: span.underline
                    )
                }
            )
        }
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

    private func lineText(_ line: WawonaTerminalLine) -> some View {
        let spans = line.spans.isEmpty
            ? [WawonaTerminalSpan(id: 0, text: " ", fg: 0xE8E8E8, bg: 0, bold: false, italic: false, underline: false)]
            : line.spans
        return HStack(spacing: 0) {
            ForEach(spans) { span in
                spanView(span)
            }
        }
        .lineLimit(1)
        .fixedSize(horizontal: true, vertical: true)
        .frame(minHeight: 16, alignment: .leading)
    }

    private func spanView(_ span: WawonaTerminalSpan) -> some View {
        HStack(spacing: 0) {
            ForEach(Array(terminalFontRuns(span.text).enumerated()), id: \.offset) { _, run in
                styled(run.0, span)
                    .font(run.1 ? Font.custom("DejaVuSansM Nerd Font Mono", size: 13) : Font.system(size: 13, design: .monospaced))
            }
        }
        .background(span.bg == 0 ? Color.clear : rgb(span.bg))
    }

    private func styled(_ raw: String, _ span: WawonaTerminalSpan) -> Text {
        var text = Text(raw).foregroundColor(rgb(span.fg))
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

private func terminalFontRuns(_ text: String) -> [(String, Bool)] {
    func nerd(_ ch: Character) -> Bool {
        !ch.unicodeScalars.isEmpty
            && ch.unicodeScalars.allSatisfy { scalar in
                let u = scalar.value
                return (0xE000...0xF8FF).contains(u)
                    || (0xF0000...0xFFFFD).contains(u)
                    || (0x100000...0x10FFFD).contains(u)
            }
    }
    var out: [(String, Bool)] = []
    var current = ""
    var flag: Bool?
    func flush() {
        guard let flag, !current.isEmpty else { return }
        out.append((current, flag))
        current = ""
    }
    for ch in text {
        let next = nerd(ch)
        if flag != next {
            flush()
            flag = next
        }
        current.append(ch)
    }
    flush()
    if out.isEmpty {
        out.append((" ", false))
    }
    return out
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
