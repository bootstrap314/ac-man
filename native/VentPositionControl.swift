import Cocoa

@MainActor
final class VentPositionControl: NSControl {
    enum Axis { case vertical, horizontal }
    private let axis: Axis
    private let buttons = NSStackView()
    private var values: [Int32] = []
    private(set) var selectedValue: Int32?
    override var isEnabled: Bool { didSet { buttons.arrangedSubviews.compactMap { $0 as? NSButton }.forEach { $0.isEnabled = isEnabled; $0.needsDisplay = true } } }
    override var mouseDownCanMoveWindow: Bool { false }

    init(axis: Axis) {
        self.axis = axis
        super.init(frame: .zero)
        buttons.orientation = .horizontal; buttons.spacing = 6; buttons.distribution = .fillEqually
        buttons.translatesAutoresizingMaskIntoConstraints = false
        addSubview(buttons)
        NSLayoutConstraint.activate([
            buttons.leadingAnchor.constraint(equalTo: leadingAnchor), buttons.trailingAnchor.constraint(equalTo: trailingAnchor),
            buttons.topAnchor.constraint(equalTo: topAnchor), buttons.bottomAnchor.constraint(equalTo: bottomAnchor),
            heightAnchor.constraint(equalToConstant: 54)
        ])
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

    func update(values: [Int32], selected: Int32?) {
        if self.values != values {
            self.values = values
            for view in buttons.arrangedSubviews { buttons.removeArrangedSubview(view); view.removeFromSuperview() }
            for value in values {
                let button = VentPositionButton(axis: axis, value: value)
                button.target = self; button.action = #selector(choose(_:)); button.isEnabled = isEnabled
                buttons.addArrangedSubview(button)
                button.heightAnchor.constraint(equalToConstant: 54).isActive = true
            }
        }
        selectedValue = values.contains(where: { $0 == selected }) ? selected : nil
        for case let button as VentPositionButton in buttons.arrangedSubviews {
            button.state = Int32(button.tag) == selectedValue ? .on : .off
            button.needsDisplay = true
        }
    }
    @objc private func choose(_ sender: NSButton) {
        guard isEnabled else { return }
        update(values: values, selected: Int32(sender.tag))
        sendAction(action, to: target)
    }
}

@MainActor
private final class VentPositionButton: NSButton {
    private let axis: VentPositionControl.Axis
    private let position: Int
    init(axis: VentPositionControl.Axis, value: Int32) {
        self.axis = axis
        position = [Int32(0), 1, 25, 50, 75, 100].firstIndex(of: value) ?? 0
        super.init(frame: .zero)
        tag = Int(value); title = ""; isBordered = false; setButtonType(.pushOnPushOff)
        let label = position == 0 ? "Off" : "Position \(position)"
        toolTip = "\(axis == .vertical ? "Vertical" : "Horizontal") vent: \(label)"
        setAccessibilityLabel(toolTip)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    override var mouseDownCanMoveWindow: Bool { false }
    override var isFlipped: Bool { false }
    override var alignmentRectInsets: NSEdgeInsets { NSEdgeInsetsZero }
    override func viewDidChangeEffectiveAppearance() { super.viewDidChangeEffectiveAppearance(); needsDisplay = true }
    override func draw(_ dirtyRect: NSRect) {
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let tile = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 7, yRadius: 7)
        NSColor.labelColor.withAlphaComponent(state == .on ? (dark ? 0.24 : 0.16) : (isHighlighted ? 0.14 : 0.06)).setFill(); tile.fill()
        NSColor.labelColor.withAlphaComponent(state == .on ? 0.36 : 0.16).setStroke(); tile.lineWidth = 0.75; tile.stroke()
        let ink = NSColor.labelColor.withAlphaComponent(isEnabled ? 1 : 0.35)
        ink.setStroke()
        let mid = bounds.midX
        let iconY = bounds.height - 22
        if position == 0 {
            let circle = NSBezierPath(ovalIn: NSRect(x: mid - 7, y: iconY - 7, width: 14, height: 14))
            circle.lineWidth = 1.4; circle.stroke()
            let slash = NSBezierPath(); slash.move(to: NSPoint(x: mid - 5, y: iconY - 5)); slash.line(to: NSPoint(x: mid + 5, y: iconY + 5)); slash.lineWidth = 1.4; slash.stroke()
        } else {
            let origin: NSPoint
            let angle: CGFloat
            let vent: NSRect
            if axis == .vertical {
                vent = NSRect(x: mid - 14, y: iconY - 8, width: 4, height: 19)
                origin = NSPoint(x: mid - 5, y: iconY + 7)
                angle = -CGFloat(position - 1) * .pi / 8
            } else {
                vent = NSRect(x: mid - 11, y: iconY + 8, width: 22, height: 4)
                origin = NSPoint(x: mid, y: iconY + 3)
                let turns: [CGFloat] = [-65, -30, 0, 30, 65]
                angle = -.pi / 2 + turns[position - 1] * .pi / 180
            }
            let bar = NSBezierPath(roundedRect: vent, xRadius: 1.5, yRadius: 1.5); bar.lineWidth = 1.3; bar.stroke()
            let end = NSPoint(x: origin.x + 17 * cos(angle), y: origin.y + 17 * sin(angle))
            let arrow = NSBezierPath(); arrow.move(to: origin); arrow.line(to: end)
            for turn: CGFloat in [-0.55, 0.55] {
                arrow.move(to: end)
                arrow.line(to: NSPoint(x: end.x - 5 * cos(angle + turn), y: end.y - 5 * sin(angle + turn)))
            }
            arrow.lineWidth = 1.5; arrow.lineCapStyle = .round; arrow.lineJoinStyle = .round; arrow.stroke()
        }
        let label = position == 0 ? "Off" : String(position)
        let attributes: [NSAttributedString.Key: Any] = [.font: NSFont.systemFont(ofSize: 11, weight: .medium), .foregroundColor: ink]
        let size = (label as NSString).size(withAttributes: attributes)
        (label as NSString).draw(at: NSPoint(x: mid - size.width / 2, y: 4), withAttributes: attributes)
    }
    override var focusRingMaskBounds: NSRect { bounds }
    override func drawFocusRingMask() { NSBezierPath(roundedRect: bounds, xRadius: 7, yRadius: 7).fill() }
}
