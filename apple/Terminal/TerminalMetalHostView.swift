#if os(iOS) || os(tvOS) || os(visionOS)
import SwiftUI
import UIKit
import MetalKit

/// SwiftUI host for product Metal Present (Rootshell-style: UIView + Metal layer).
struct TerminalMetalHostView: UIViewRepresentable {
  var cells: [WwnTermCell]
  var cols: Int
  var rows: Int
  var blinkOn: Bool
  var cellW: CGFloat
  var cellH: CGFloat
  var onPointer: ((UInt32, UInt32, UInt8, Bool, Bool) -> Void)?

  func makeUIView(context: Context) -> TerminalMetalTouchHost {
    let host = TerminalMetalTouchHost(frame: .zero)
    host.onPointer = onPointer
    return host
  }

  func updateUIView(_ uiView: TerminalMetalTouchHost, context: Context) {
    uiView.onPointer = onPointer
    uiView.present(
      cells, count: cells.count, cols: cols, rows: rows, blinkOn: blinkOn,
      cellW: cellW, cellH: cellH
    )
  }
}

/// Touch + scale wrapper around `TerminalMetalPresent`.
final class TerminalMetalTouchHost: UIView {
  let gpu = TerminalMetalPresent(frame: .zero, device: nil)
  var onPointer: ((UInt32, UInt32, UInt8, Bool, Bool) -> Void)?
  private var cols: Int = 80
  private var rows: Int = 24
  private var cellW: CGFloat = 8
  private var cellH: CGFloat = 16

  override init(frame: CGRect) {
    super.init(frame: frame)
    backgroundColor = .black
    isMultipleTouchEnabled = false
    gpu.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    gpu.frame = bounds
    addSubview(gpu)
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) { fatalError("unused") }

  override func layoutSubviews() {
    super.layoutSubviews()
    gpu.frame = bounds
    let scale = window?.screen.scale ?? UIScreen.main.scale
    gpu.syncBackingScale(scale)
    gpu.drawableSize = CGSize(
      width: max(1, bounds.width * scale), height: max(1, bounds.height * scale)
    )
  }

  func present(
    _ grid: [WwnTermCell], count: Int, cols: Int, rows: Int, blinkOn: Bool,
    cellW: CGFloat, cellH: CGFloat
  ) {
    self.cols = max(1, cols)
    self.rows = max(1, rows)
    self.cellW = max(1, cellW)
    self.cellH = max(1, cellH)
    gpu.present(
      grid, count: count, cols: self.cols, rows: self.rows, blinkOn: blinkOn,
      cellW: self.cellW, cellH: self.cellH
    )
  }

  private func cellAt(_ point: CGPoint) -> (UInt32, UInt32)? {
    guard cellW > 0, cellH > 0, bounds.width > 0 else { return nil }
    let col = UInt32(min(max(0, Int(point.x / cellW)), cols - 1))
    let row = UInt32(min(max(0, Int(point.y / cellH)), rows - 1))
    return (col, row)
  }

  override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
    guard let p = touches.first.map({ $0.location(in: self) }), let cell = cellAt(p) else { return }
    onPointer?(cell.0, cell.1, 1, true, false)
  }

  override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
    guard let p = touches.first.map({ $0.location(in: self) }), let cell = cellAt(p) else { return }
    onPointer?(cell.0, cell.1, 1, true, true)
  }

  override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
    guard let p = touches.first.map({ $0.location(in: self) }), let cell = cellAt(p) else { return }
    onPointer?(cell.0, cell.1, 1, false, false)
  }

  override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
    touchesEnded(touches, with: event)
  }
}
#endif
