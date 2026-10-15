import Cocoa

/// A stable blurred surface; focus changes never fade the controls or background.
@MainActor
final class ClimateBackgroundView: NSView {
    let contentContainer = NSView()
    private let effect = NSVisualEffectView()
    private var accessibilityObserver: NSObjectProtocol?

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        effect.blendingMode = .behindWindow
        effect.state = .active
        effect.autoresizingMask = [.width, .height]
        addSubview(effect)
        contentContainer.wantsLayer = true
        contentContainer.autoresizingMask = [.width, .height]
        addSubview(contentContainer)
        accessibilityObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification,
            object: nil, queue: .main
        ) { [weak self] _ in
            Task { @MainActor in self?.updateSurface() }
        }
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
    }

    private func updateSurface() {
        let solid = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
            || NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        effect.material = dark ? .hudWindow : .popover
        effect.isHidden = solid
        // Tint only the background layer, never the opacity of the controls.
        effectiveAppearance.performAsCurrentDrawingAppearance {
            let tint = dark
                ? NSColor(calibratedWhite: 0.12, alpha: 0.55)
                : NSColor(calibratedRed: 0.91, green: 0.95, blue: 1, alpha: 0.55)
            contentContainer.layer?.backgroundColor = (solid ? NSColor.windowBackgroundColor : tint).cgColor
        }
    }
}
