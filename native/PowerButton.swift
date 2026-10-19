import Cocoa

/// A symbol-only toggle with an explicit selected state in both appearances.
@MainActor
final class PowerButton: NSButton {
    private var hasKnownState = false
    private var connectionBroken = false
    private var flashGreen = false
    private var connectionTimer: Timer?
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        title = ""
        isBordered = false
        setButtonType(.pushOnPushOff)
        imagePosition = .imageOnly
        image = NSImage(systemSymbolName: "power", accessibilityDescription: nil)?
            .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 18, weight: .semibold))
        setPowerState(nil)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    func setPowerState(_ isOn: Bool?) {
        hasKnownState = isOn != nil
        state = isOn == true ? .on : .off
        contentTintColor = hasKnownState ? .white : .labelColor
        toolTip = isOn.map { $0 ? "Turn off" : "Turn on" } ?? "Power unavailable"
        setAccessibilityLabel(isOn.map { $0 ? "Turn air conditioner off" : "Turn air conditioner on" } ?? "Power unavailable")
        setAccessibilityValue(isOn.map { $0 ? "On" : "Off" } ?? "Unknown")
        needsDisplay = true
    }

    func setConnectionBroken(_ broken: Bool) {
        if broken != connectionBroken {
            connectionBroken = broken
            connectionTimer?.invalidate()
            connectionTimer = nil
            flashGreen = false
            if broken {
                let timer = Timer(timeInterval: 0.8, repeats: true) { [weak self] _ in
                    Task { @MainActor [weak self] in
                        guard let self, self.connectionBroken else { return }
                        self.flashGreen.toggle()
                        self.needsDisplay = true
                    }
                }
                connectionTimer = timer
                RunLoop.main.add(timer, forMode: .common)
            }
        }
        if broken {
            contentTintColor = .white
            toolTip = "Connection unavailable"
            setAccessibilityLabel("Air conditioner connection unavailable")
            setAccessibilityValue("Connection unavailable")
        }
        needsDisplay = true
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override var isFlipped: Bool { false }

    override var alignmentRectInsets: NSEdgeInsets { NSEdgeInsetsZero }

    override var intrinsicContentSize: NSSize { NSSize(width: 38, height: 38) }

    override func draw(_ dirtyRect: NSRect) {
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        var fill = hasKnownState ? (state == .on ? NSColor.systemGreen : NSColor.systemRed)
            : NSColor(calibratedWhite: dark ? 0.28 : 0.94, alpha: 1)
        if connectionBroken { fill = flashGreen ? .systemGreen : .systemRed }
        if isHighlighted { fill = fill.blended(withFraction: 0.12, of: .black) ?? fill }
        if !isEnabled && !connectionBroken { fill = fill.withAlphaComponent(0.45) }
        let diameter = max(0, min(bounds.width, bounds.height) - 2)
        let circle = NSBezierPath(ovalIn: NSRect(
            x: bounds.midX - diameter / 2, y: bounds.midY - diameter / 2,
            width: diameter, height: diameter))
        fill.setFill(); circle.fill()
        NSColor.white.withAlphaComponent(dark ? 0.12 : 0.55).setStroke()
        circle.lineWidth = 0.75; circle.stroke()
        // Center the visible path itself, avoiding NSButtonCell/SF Symbol insets.
        let symbol = NSBezierPath()
        symbol.appendArc(withCenter: .zero, radius: 8, startAngle: 45, endAngle: 135, clockwise: true)
        symbol.move(to: NSPoint(x: 0, y: 1))
        symbol.line(to: NSPoint(x: 0, y: 10))
        var transform = AffineTransform()
        transform.translate(x: bounds.midX - symbol.bounds.midX, y: bounds.midY - symbol.bounds.midY)
        symbol.transform(using: transform)
        (hasKnownState || connectionBroken ? NSColor.white : NSColor.labelColor)
            .withAlphaComponent(isEnabled || connectionBroken ? 1 : 0.45).setStroke()
        symbol.lineWidth = 2.2
        symbol.lineCapStyle = .round
        symbol.stroke()
    }
}
