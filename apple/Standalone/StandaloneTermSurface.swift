#if os(macOS)
import AppKit
import CoreText
import Darwin
import Metal
import MetalKit

// MARK: - C ABI (libghostty-rs)

@_silgen_name("wawona_pty_spawn_shell")
private func wawona_pty_spawn_shell(_ master: UnsafeMutablePointer<Int32>, _ shell: UnsafePointer<CChar>?) -> pid_t

@_silgen_name("ghostty_surface_new")
private func ghostty_surface_new(_ cols: UInt32, _ rows: UInt32) -> OpaquePointer?
@_silgen_name("ghostty_surface_free")
private func ghostty_surface_free(_ term: OpaquePointer?)
@_silgen_name("ghostty_surface_resize")
private func ghostty_surface_resize(_ term: OpaquePointer?, _ cols: UInt32, _ rows: UInt32)
@_silgen_name("ghostty_surface_write")
private func ghostty_surface_write(_ term: OpaquePointer?, _ bytes: UnsafePointer<UInt8>?, _ len: Int)
@_silgen_name("ghostty_surface_viewport")
private func ghostty_surface_viewport(
    _ term: OpaquePointer?, _ origin: UInt32, _ out: UnsafeMutablePointer<WwnTermCell>?, _ cap: UInt32,
    _ cols: UnsafeMutablePointer<UInt32>?, _ rows: UnsafeMutablePointer<UInt32>?
) -> UInt32
@_silgen_name("ghostty_surface_origin")
private func ghostty_surface_origin(_ term: OpaquePointer?) -> UInt32
@_silgen_name("ghostty_surface_take_reply")
private func ghostty_surface_take_reply(
    _ term: OpaquePointer?, _ buf: UnsafeMutablePointer<UInt8>?, _ cap: Int
) -> Int
@_silgen_name("ghostty_surface_pointer")
private func ghostty_surface_pointer(
    _ term: OpaquePointer?, _ button: UInt8, _ col: UInt32, _ row: UInt32, _ pressed: Int32, _ motion: Int32
)
@_silgen_name("ghostty_surface_set_cell_px")
private func ghostty_surface_set_cell_px(_ term: OpaquePointer?, _ width: UInt32, _ height: UInt32)

/// Must match Rust `#[repr(C)] WwnTermCell` / `GhosttyCell` (4×u32).
struct WwnTermCell {
    var cp: UInt32
    var fg: UInt32
    var bg: UInt32
    var attrs: UInt32
}

private func ghosttyViewport(
    _ term: OpaquePointer, origin: UInt32, cols: inout UInt32, rows: inout UInt32
) -> [WwnTermCell] {
    var outCols: UInt32 = 0
    var outRows: UInt32 = 0
    // Probe size with a tiny cap first? ABI needs full grid capacity.
    let probeCap: UInt32 = 512 * 256
    var storage = [WwnTermCell](
        repeating: WwnTermCell(cp: 32, fg: 0xE8E8E8, bg: 0, attrs: 0),
        count: Int(probeCap)
    )
    let n = storage.withUnsafeMutableBufferPointer { buf -> UInt32 in
        ghostty_surface_viewport(
            term, origin, buf.baseAddress, probeCap, &outCols, &outRows
        )
    }
    cols = outCols
    rows = outRows
    let count = Int(n)
    guard count > 0, count <= storage.count else { return [] }
    return Array(storage.prefix(count))
}

struct TermPicture {
    var col: Int32 = 0
    var row: Int32 = 0
    var xPx: Int32 = 0
    var yPx: Int32 = 0
    var cols: Int = 0
    var rows: Int = 0
    var z: Int32 = 0
    var pxW: Int = 0
    var pxH: Int = 0
    var rgba: Data = Data()
}

// MARK: - Metal GPU paint (same path as Wawona GhosttyGpu)

private struct GpuDim {
    var cols: UInt32
    var rows: UInt32
    var phase: UInt32
}

private struct GpuCell {
    var slot: UInt32
    var fg: UInt32
    var bg: UInt32
    var attr: UInt32
}

final class GhosttyGpuView: MTKView, MTKViewDelegate {
    private var pipeline: MTLRenderPipelineState?
    private var queue: MTLCommandQueue?
    private var atlas: MTLTexture?
    private var cells = [GpuCell]()
    private var cols = 1
    private var rows = 1
    private var blinkOn = true
    private var cache: [UInt32: UInt32] = [:]
    private var nextSlot: UInt32 = 1
    private let atlasSide = 16
    private let glyphW: Int = 16
    private let glyphH: Int = 32
    private var cellW: Float = 8
    private var cellH: Float = 16

    override init(frame frameRect: NSRect, device: MTLDevice?) {
        let device = device ?? MTLCreateSystemDefaultDevice()
        super.init(frame: frameRect, device: device)
        delegate = self
        isPaused = true
        enableSetNeedsDisplay = true
        colorPixelFormat = .bgra8Unorm
        clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 1)
        framebufferOnly = true
        guard let device else { return }
        queue = device.makeCommandQueue()
        atlas = Self.makeAtlas(device, glyphW: glyphW, glyphH: glyphH, atlasSide: atlasSide)
        pipeline = Self.makePipeline(
            device, pixelFormat: colorPixelFormat, atlasSide: atlasSide, glyphW: glyphW)
    }

    @available(*, unavailable)
    required init(coder: NSCoder) { fatalError("unused") }

    override var acceptsFirstResponder: Bool { false }
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    func present(
        _ grid: [WwnTermCell], count: Int, cols: Int, rows: Int, blinkOn: Bool,
        cellW: CGFloat, cellH: CGFloat
    ) {
        self.cols = max(1, cols)
        self.rows = max(1, rows)
        self.blinkOn = blinkOn
        self.cellW = Float(max(cellW, 1))
        self.cellH = Float(max(cellH, 1))
        let n = min(count, cols * rows)
        if cells.count != n { cells = Array(repeating: GpuCell(slot: 0, fg: 0, bg: 0, attr: 0), count: n) }
        for i in 0..<n {
            let cell = grid[i]
            cells[i] = GpuCell(slot: slot(cell.cp), fg: cell.fg, bg: cell.bg, attr: cell.attrs)
        }
        setNeedsDisplay(bounds)
    }

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    func draw(in view: MTKView) {
        guard !cells.isEmpty, let device, let drawable = currentDrawable,
            let pass = currentRenderPassDescriptor, let queue, let pipeline, let atlas
        else { return }
        let buffer = cells.withUnsafeBytes { raw -> MTLBuffer? in
            guard let base = raw.baseAddress else { return nil }
            return device.makeBuffer(bytes: base, length: raw.count, options: .storageModeShared)
        }
        guard let buffer, let cmd = queue.makeCommandBuffer(),
            let enc = cmd.makeRenderCommandEncoder(descriptor: pass)
        else { return }
        var dim = GpuDim(cols: UInt32(cols), rows: UInt32(rows), phase: blinkOn ? 1 : 0)
        enc.setRenderPipelineState(pipeline)
        enc.setVertexBuffer(buffer, offset: 0, index: 0)
        enc.setVertexBytes(&dim, length: MemoryLayout<GpuDim>.size, index: 1)
        enc.setFragmentTexture(atlas, index: 0)
        enc.setFragmentBytes(&dim, length: MemoryLayout<GpuDim>.size, index: 0)
        enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6, instanceCount: cells.count)
        enc.endEncoding()
        cmd.present(drawable)
        cmd.commit()
    }

    private func slot(_ cp: UInt32) -> UInt32 {
        if cp < 33 { return 0 }
        if let hit = cache[cp] { return hit }
        if nextSlot >= UInt32(atlasSide * atlasSide) {
            cache.removeAll()
            nextSlot = 1
            clearAtlas()
        }
        let slot = nextSlot
        nextSlot += 1
        cache[cp] = slot
        stamp(cp, slot: slot)
        return slot
    }

    private func clearAtlas() {
        guard let atlas else { return }
        let bytes = glyphW * glyphH
        let zeros = [UInt8](repeating: 0, count: bytes)
        for row in 0..<atlasSide {
            for col in 0..<atlasSide {
                let region = MTLRegionMake2D(col * glyphW, row * glyphH, glyphW, glyphH)
                zeros.withUnsafeBytes { raw in
                    guard let base = raw.baseAddress else { return }
                    atlas.replace(region: region, mipmapLevel: 0, withBytes: base, bytesPerRow: glyphW)
                }
            }
        }
    }

    private func stamp(_ cp: UInt32, slot: UInt32) {
        guard let atlas, let scalar = UnicodeScalar(cp) else { return }
        let w = glyphW
        let h = glyphH
        var pixels = [UInt8](repeating: 0, count: w * h)
        let pointSize = CGFloat(h) * 0.72
        let font = NSFont.monospacedSystemFont(ofSize: pointSize, weight: .regular)
        let attrs: [NSAttributedString.Key: Any] = [
            .font: font,
            .foregroundColor: NSColor.white,
        ]
        let attr = NSAttributedString(string: String(Character(scalar)), attributes: attrs)
        let line = CTLineCreateWithAttributedString(attr)
        pixels.withUnsafeMutableBytes { raw in
            guard let base = raw.baseAddress,
                let space = CGColorSpace(name: CGColorSpace.linearGray),
                let ctx = CGContext(
                    data: base, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w,
                    space: space, bitmapInfo: CGImageAlphaInfo.none.rawValue)
            else { return }
            // Top-left origin matching Metal (same as Wawona GhosttyGpu).
            ctx.translateBy(x: 0, y: CGFloat(h))
            ctx.scaleBy(x: 1, y: -1)
            ctx.setShouldAntialias(true)
            ctx.setAllowsAntialiasing(true)
            let baseline = max(CGFloat(1), -font.descender + 1)
            ctx.textPosition = CGPoint(x: 1, y: baseline)
            CTLineDraw(line, ctx)
        }
        let region = MTLRegionMake2D(
            Int(slot % UInt32(atlasSide)) * w, Int(slot / UInt32(atlasSide)) * h, w, h
        )
        pixels.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return }
            atlas.replace(region: region, mipmapLevel: 0, withBytes: base, bytesPerRow: w)
        }
    }

    private static func makeAtlas(
        _ device: MTLDevice, glyphW: Int, glyphH: Int, atlasSide: Int
    ) -> MTLTexture? {
        let desc = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .r8Unorm,
            width: glyphW * atlasSide,
            height: glyphH * atlasSide,
            mipmapped: false
        )
        desc.usage = [.shaderRead]
        return device.makeTexture(descriptor: desc)
    }

    private static func makePipeline(
        _ device: MTLDevice, pixelFormat: MTLPixelFormat, atlasSide: Int, glyphW: Int
    ) -> MTLRenderPipelineState? {
        let side = UInt32(atlasSide)
        let boldUV = String(format: "%.8f", 1.0 / Double(glyphW * atlasSide))
        let src = """
        #include <metal_stdlib>
        using namespace metal;
        struct Cell { uint slot; uint fg; uint bg; uint attr; };
        struct VOut { float4 pos [[position]]; float2 uv; float4 fg; float4 bg; uint attr [[flat]]; };
        struct Dim { uint cols; uint rows; uint phase; };
        float4 unpack(uint c) {
          return float4(float((c >> 16) & 255) / 255.0, float((c >> 8) & 255) / 255.0, float(c & 255) / 255.0, 1);
        }
        vertex VOut ghostty_term_vs(uint vid [[vertex_id]], uint iid [[instance_id]],
                                constant Cell *cells [[buffer(0)]], constant Dim &d [[buffer(1)]]) {
          float2 q[6] = { float2(0,0), float2(1,0), float2(0,1), float2(1,0), float2(1,1), float2(0,1) };
          Cell cell = cells[iid];
          float2 g = q[vid];
          if ((cell.attr & 4u) != 0u) g.x += (0.5 - g.y) * 0.25;
          uint col = iid % max(d.cols, 1u);
          uint row = iid / max(d.cols, 1u);
          float2 step = float2(2.0 / float(max(d.cols, 1u)), 2.0 / float(max(d.rows, 1u)));
          float2 ndc = float2(-1.0, 1.0) + float2(float(col) + g.x, -(float(row) + g.y)) * step;
          uint slot = cell.slot;
          float side = \(side).0;
          // Stamp atlas is top-down; sample with flipped V so glyphs read upright.
          float2 uv = (float2(slot % \(side)u, slot / \(side)u) + float2(g.x, 1.0 - g.y)) / side;
          VOut o; o.pos = float4(ndc, 0, 1); o.uv = uv; o.fg = unpack(cell.fg); o.bg = unpack(cell.bg); o.attr = cell.attr;
          return o;
        }
        fragment float4 ghostty_term_fs(VOut in [[stage_in]], texture2d<float> atlas [[texture(0)]],
                                    constant Dim &d [[buffer(0)]]) {
          constexpr sampler s(filter::nearest, address::clamp_to_edge);
          float m = atlas.sample(s, in.uv).r;
          if ((in.attr & 1u) != 0u) m = max(m, atlas.sample(s, in.uv + float2(\(boldUV), 0)).r);
          if ((in.attr & 16u) != 0u && d.phase == 0u) m = 0.0;
          float4 c = mix(in.bg, in.fg, step(0.28, m));
          float2 local = float2(fract(in.uv.x * \(side).0), 1.0 - fract(in.uv.y * \(side).0));
          if ((in.attr & 8u) != 0u && local.y > 0.86) c = in.fg;
          if ((in.attr & 32u) != 0u && abs(local.y - 0.5) < 0.07) c = in.fg;
          return c;
        }
        """
        guard let lib = try? device.makeLibrary(source: src, options: nil),
            let vs = lib.makeFunction(name: "ghostty_term_vs"),
            let fs = lib.makeFunction(name: "ghostty_term_fs")
        else { return nil }
        let desc = MTLRenderPipelineDescriptor()
        desc.vertexFunction = vs
        desc.fragmentFunction = fs
        desc.colorAttachments[0].pixelFormat = pixelFormat
        return try? device.makeRenderPipelineState(descriptor: desc)
    }
}

// MARK: - Host surface + PTY

final class StandaloneTermSurface: NSView {
    var onReply: ((Data) -> Void)?

    private var term: OpaquePointer?
    private let gpu = GhosttyGpuView(frame: .zero, device: nil)
    private var grid = [WwnTermCell](repeating: WwnTermCell(cp: 32, fg: 0xE8E8E8, bg: 0, attrs: 0), count: 512 * 32)
    private var cols: UInt32 = 100
    private var rows: UInt32 = 32
    private var blinkOn = true
    private var blinkTimer: Timer?
    private let pointSize: CGFloat = 13
    private var cellPxW: CGFloat = 8
    private var cellPxH: CGFloat = 16

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor
        term = ghostty_surface_new(cols, rows)
        gpu.autoresizingMask = [.width, .height]
        gpu.frame = bounds
        addSubview(gpu)
        blinkTimer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.blinkOn.toggle()
            self.redraw()
        }
        let banner = "\u{1b}[1;36mWawona Terminal\u{1b}[0m  standalone · libghostty-rs · Metal\r\n"
        feed(Data(banner.utf8))
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    deinit {
        blinkTimer?.invalidate()
        if let term { ghostty_surface_free(term) }
    }

    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func hitTest(_ point: NSPoint) -> NSView? { bounds.contains(point) ? self : nil }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }

    func feed(_ bytes: Data) {
        guard let term, !bytes.isEmpty else { return }
        bytes.withUnsafeBytes { raw in
            guard let base = raw.baseAddress?.assumingMemoryBound(to: UInt8.self) else { return }
            ghostty_surface_write(term, base, bytes.count)
        }
        flushReply()
        redraw()
    }

    override func layout() {
        super.layout()
        gpu.frame = bounds
        syncDrawableScale()
        fit()
    }

    override func viewDidChangeBackingProperties() {
        super.viewDidChangeBackingProperties()
        syncDrawableScale()
    }

    private func syncDrawableScale() {
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2
        gpu.layer?.contentsScale = scale
        let w = max(1, bounds.width * scale)
        let h = max(1, bounds.height * scale)
        gpu.drawableSize = CGSize(width: w, height: h)
    }

    override func keyDown(with event: NSEvent) {
        if event.modifierFlags.contains(.command) {
            super.keyDown(with: event)
            return
        }
        let chars = event.characters ?? event.charactersIgnoringModifiers ?? ""
        if event.keyCode == 36 || event.keyCode == 76 {
            onReply?(Data([0x0D]))
            return
        }
        if event.keyCode == 51 {
            onReply?(Data([0x7F]))
            return
        }
        if !chars.isEmpty {
            onReply?(Data(chars.utf8))
        }
    }

    private func fit() {
        guard let term, bounds.width > 1, bounds.height > 1 else { return }
        let font = NSFont.monospacedSystemFont(ofSize: pointSize, weight: .regular)
        let cellW = max(1, ("M" as NSString).size(withAttributes: [.font: font]).width)
        let cellH = max(1, font.ascender - font.descender + font.leading)
        let nextCols = UInt32(min(512, max(20, Int((bounds.width - 16) / cellW))))
        let nextRows = UInt32(min(256, max(8, Int(bounds.height / cellH))))
        cellPxW = cellW
        cellPxH = cellH
        ghostty_surface_set_cell_px(term, UInt32(max(1, cellW.rounded())), UInt32(max(1, cellH.rounded())))
        guard nextCols != cols || nextRows != rows else {
            redraw()
            return
        }
        cols = nextCols
        rows = nextRows
        ghostty_surface_resize(term, cols, rows)
        redraw()
    }

    private func flushReply() {
        guard let term else { return }
        var buf = [UInt8](repeating: 0, count: 256)
        let n = buf.withUnsafeMutableBytes { raw in
            ghostty_surface_take_reply(term, raw.baseAddress?.assumingMemoryBound(to: UInt8.self), raw.count)
        }
        if n > 0 { onReply?(Data(buf.prefix(n))) }
    }

    private func redraw() {
        guard let term else { return }
        var outCols: UInt32 = 0
        var outRows: UInt32 = 0
        let cells = ghosttyViewport(
            term, origin: ghostty_surface_origin(term), cols: &outCols, rows: &outRows
        )
        if outCols > 0 { cols = outCols }
        if outRows > 0 { rows = outRows }
        grid = cells.isEmpty
            ? [WwnTermCell(cp: 32, fg: 0xE8E8E8, bg: 0, attrs: 0)]
            : cells
        gpu.present(
            grid, count: grid.count, cols: Int(cols), rows: Int(rows), blinkOn: blinkOn,
            cellW: cellPxW, cellH: cellPxH
        )
    }
}

final class StandaloneAppDelegate: NSObject, NSApplicationDelegate {
    private var window: NSWindow?
    private var surface: StandaloneTermSurface?
    private var master: Int32 = -1
    private var child: pid_t = -1
    private var reader: DispatchSourceRead?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let surface = StandaloneTermSurface(frame: NSRect(x: 0, y: 0, width: 900, height: 560))
        surface.onReply = { [weak self] data in
            self?.writeMaster(data)
        }
        self.surface = surface

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 900, height: 560),
            styleMask: [.titled, .closable, .resizable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.title = "Wawona Terminal"
        window.contentView = surface
        window.center()
        window.makeKeyAndOrderFront(nil)
        self.window = window

        startShell()
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    private func startShell() {
        var m: Int32 = -1
        let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
        let pid = shell.withCString { path in
            wawona_pty_spawn_shell(&m, path)
        }
        guard pid > 0, m >= 0 else { return }
        master = m
        child = pid
        let flags = fcntl(master, F_GETFL)
        _ = fcntl(master, F_SETFL, flags | O_NONBLOCK)

        let source = DispatchSource.makeReadSource(fileDescriptor: master, queue: .main)
        source.setEventHandler { [weak self] in
            self?.drainMaster()
        }
        source.resume()
        reader = source
    }

    private func drainMaster() {
        guard master >= 0 else { return }
        var buf = [UInt8](repeating: 0, count: 8192)
        while true {
            let n = read(master, &buf, buf.count)
            if n > 0 {
                surface?.feed(Data(buf.prefix(n)))
            } else {
                break
            }
        }
    }

    private func writeMaster(_ data: Data) {
        guard master >= 0, !data.isEmpty else { return }
        data.withUnsafeBytes { raw in
            if let base = raw.baseAddress {
                _ = write(master, base, data.count)
            }
        }
    }
}

@main
enum StandaloneMain {
    static func main() {
        let app = NSApplication.shared
        let delegate = StandaloneAppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}
#endif
