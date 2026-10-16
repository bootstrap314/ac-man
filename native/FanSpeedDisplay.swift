import Foundation

/// Device status may contain reserved values; never present them as percentages.
enum FanSpeedDisplay {
    static func label(_ speed: Int32, boost: Bool) -> String {
        if boost { return "Boost" }
        if speed == 102 { return "Auto" }
        return (0...100).contains(speed) ? "\(speed)%" : "—"
    }

    static func sliderValue(_ speed: Int32, lastManual: Int?) -> Double {
        if (1...100).contains(speed) { return Double(speed) }
        return Double(min(100, max(1, lastManual ?? 50)))
    }
}
