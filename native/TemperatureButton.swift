import Cocoa

/// Draw the symbol around the circle's center instead of a text baseline.
@MainActor
final class TemperatureButton: NSButton {
    private let increasesTemperature: Bool

    init(increasesTemperature: Bool) {
        self.increasesTemperature = increasesTemperature
        super.init(frame: .zero)
        title = ""
        isBordered = false
        setButtonType(.momentaryChange)
        setAccessibilityLabel(increasesTemperature ? "Increase temperature" : "Decrease temperature")
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    override var alignmentRectInsets: NSEdgeInsets { NSEdgeInsetsZero }
    override var intrinsicContentSize: NSSize { NSSize(width: 64, height: 64) }

    private var circleRect: NSRect {
        let diameter = max(0, min(bounds.width, bounds.height) - 2)
        return NSRect(x: bounds.midX - diameter / 2, y: bounds.midY - diameter / 2,
                      width: diameter, height: diameter)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let circle = NSBezierPath(ovalIn: circleRect)
        let fill = NSColor.white.withAlphaComponent(isHighlighted ? (dark ? 0.20 : 0.70) : (dark ? 0.08 : 0.35))
        fill.setFill(); circle.fill()
        NSColor.labelColor.withAlphaComponent(dark ? 0.22 : 0.18).setStroke()
        circle.lineWidth = 0.75; circle.stroke()

        let symbol = NSBezierPath()
        let halfLength: CGFloat = 10.5
        symbol.move(to: NSPoint(x: bounds.midX - halfLength, y: bounds.midY))
        symbol.line(to: NSPoint(x: bounds.midX + halfLength, y: bounds.midY))
        if increasesTemperature {
            symbol.move(to: NSPoint(x: bounds.midX, y: bounds.midY - halfLength))
            symbol.line(to: NSPoint(x: bounds.midX, y: bounds.midY + halfLength))
        }
        symbol.lineWidth = 3.5
        symbol.lineCapStyle = .round
        let color: NSColor = increasesTemperature ? .systemRed : .systemBlue
        color.withAlphaComponent(isEnabled ? 1 : 0.35).setStroke()
        symbol.stroke()
    }

    override var focusRingMaskBounds: NSRect { circleRect }

    override func drawFocusRingMask() {
        NSBezierPath(ovalIn: circleRect).fill()
    }
}
