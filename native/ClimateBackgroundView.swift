import Cocoa

/// Keeps controls opaque while the system renders the surface behind them.
@MainActor
final class ClimateBackgroundView: NSView {
    let contentContainer = NSView()
    private let effect = NSVisualEffectView()
    private var accessibilityObserver: NSObjectProtocol?
    #if compiler(>=6.2)
    private var glass: NSView?
    #endif

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        effect.blendingMode = .behindWindow
        effect.state = .active
        addSubview(effect)
        #if compiler(>=6.2)
        if #available(macOS 26.0, *) {
            let surface = NSGlassEffectView()
            surface.cornerRadius = 14
            addSubview(surface)
            glass = surface
        }
        #endif
        accessibilityObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            Task { @MainActor in self?.updateSurface() }
        }
        addSubview(contentContainer)
        updateSurface()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    deinit {
        if let accessibilityObserver { NSWorkspace.shared.notificationCenter.removeObserver(accessibilityObserver) }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateSurface()
    }

    override func layout() {
        super.layout()
        effect.frame = bounds
        contentContainer.frame = bounds
        #if compiler(>=6.2)
        glass?.frame = bounds
        #endif
    }

    private func updateSurface() {
        let solid = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
            || NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        effectiveAppearance.performAsCurrentDrawingAppearance {
            layer?.backgroundColor = solid ? NSColor.windowBackgroundColor.cgColor : NSColor.clear.cgColor
        }
        effect.material = dark ? .underWindowBackground : .popover
        effect.isHidden = solid
        #if compiler(>=6.2)
        if #available(macOS 26.0, *), let surface = glass as? NSGlassEffectView {
            surface.style = dark ? .clear : .regular
            surface.tintColor = dark ? NSColor.black.withAlphaComponent(0.08) : NSColor.systemBlue.withAlphaComponent(0.035)
            if !solid && surface.contentView !== contentContainer {
                surface.contentView = contentContainer
            } else if solid && contentContainer.superview !== self {
                surface.contentView = nil
                addSubview(contentContainer)
            }
            surface.isHidden = solid
            effect.isHidden = true
        }
        #endif
    }
}
