import Cocoa

/// A symbol-only toggle with an explicit selected state in both appearances.
@MainActor
final class PowerButton: NSButton {
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
        state = isOn == true ? .on : .off
        contentTintColor = isOn == true ? .white : .labelColor
        toolTip = isOn.map { $0 ? "Turn off" : "Turn on" } ?? "Power unavailable"
        setAccessibilityLabel(isOn.map { $0 ? "Turn air conditioner off" : "Turn air conditioner on" } ?? "Power unavailable")
        setAccessibilityValue(isOn.map { $0 ? "On" : "Off" } ?? "Unknown")
        needsDisplay = true
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        var fill = state == .on ? NSColor.systemBlue
            : NSColor(calibratedWhite: dark ? 0.28 : 0.94, alpha: 1)
        if isHighlighted { fill = fill.blended(withFraction: 0.12, of: .black) ?? fill }
        if !isEnabled { fill = fill.withAlphaComponent(0.45) }
        let circle = NSBezierPath(ovalIn: bounds.insetBy(dx: 1, dy: 1))
        fill.setFill(); circle.fill()
        NSColor.white.withAlphaComponent(dark ? 0.12 : 0.55).setStroke()
        circle.lineWidth = 0.75; circle.stroke()
        super.draw(dirtyRect)
    }
}
