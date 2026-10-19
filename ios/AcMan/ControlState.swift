import Foundation
import AcManCore

extension SettingsChange {
    static var empty: Self { .init(power: nil, temperature: nil, mode: nil, fan: nil, swing: nil, boost: nil, eco: nil, verticalAngle: nil, horizontalAngle: nil) }
    func merging(_ next: Self) -> Self {
        .init(power: next.power ?? power, temperature: next.temperature ?? temperature,
              mode: next.mode ?? mode, fan: next.fan ?? fan, swing: next.swing ?? swing,
              boost: next.boost ?? boost, eco: next.eco ?? eco,
              verticalAngle: next.verticalAngle ?? verticalAngle, horizontalAngle: next.horizontalAngle ?? horizontalAngle)
    }
    static func preset(_ preset: FanPreset, enabled: Bool, manual: Int32, supportsBoost: Bool, supportsEco: Bool) -> Self {
        var change = Self.empty
        switch preset {
        case .automatic: change.fan = enabled ? 102 : manual
        case .boost: change.boost = enabled
        case .eco: change.eco = enabled
        }
        if enabled {
            if preset != .automatic { change.fan = manual }
            if supportsBoost { change.boost = preset == .boost }
            if supportsEco { change.eco = preset == .eco }
        }
        return change
    }
}
enum FanPreset { case automatic, boost, eco }
func fanPercentage(_ raw: Int32) -> Int32 { (1...100).contains(raw) ? raw : 50 }
func temperatureText(_ value: Double?) -> String {
    guard let value else { return "—" }
    return value.rounded() == value ? String(Int(value)) : String(format: "%.1f", value)
}
