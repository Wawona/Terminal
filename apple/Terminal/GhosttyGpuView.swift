#if os(macOS) || os(iOS) || os(tvOS) || os(visionOS)
import MetalKit

/// Compatibility alias. Product paint is `TerminalMetalPresent`.
typealias GhosttyGpuView = TerminalMetalPresent
#endif
