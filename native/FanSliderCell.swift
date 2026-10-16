import Cocoa

/// Keep the thumb opaque while AppKit tracks the slider over a blurred window.
@MainActor
final class FanSliderCell: NSSliderCell {
    override func drawKnob(_ knobRect: NSRect) {
        let diameter = min(knobRect.width, knobRect.height) - 2
        let rect = NSRect(x: knobRect.midX - diameter / 2, y: knobRect.midY - diameter / 2,
                          width: diameter, height: diameter)
        let knob = NSBezierPath(ovalIn: rect)
        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow()
        shadow.shadowColor = NSColor.black.withAlphaComponent(isEnabled ? 0.22 : 0.08)
        shadow.shadowBlurRadius = 2
        shadow.shadowOffset = NSSize(width: 0, height: -1)
        shadow.set()
        (isEnabled ? NSColor.white : NSColor(calibratedWhite: 0.65, alpha: 1)).setFill()
        knob.fill()
        NSGraphicsContext.restoreGraphicsState()
        NSColor.black.withAlphaComponent(0.12).setStroke()
        knob.lineWidth = 0.5
        knob.stroke()
    }
}
