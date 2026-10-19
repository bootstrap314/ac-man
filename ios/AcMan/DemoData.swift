import AcManCore

enum DemoData {
    static let snapshot = AppSnapshot(signedIn: true, devices: [device(1, "Living room", true), device(2, "Bedroom", false), device(3, "Office", true)], route: "")
    static func device(_ number: UInt32, _ label: String, _ power: Bool) -> DeviceSnapshot {
        let angles = [1,25,50,75,100].map { Choice(value: Int32($0), name: "\($0)") }
        return DeviceSnapshot(id: "sample-\(number)", localId: number, label: label, mac: "", connected: true, online: true, error: "", updated: 0, power: power, temperature: 23, indoor: 24, outdoor: nil, mode: 2, fan: 45, swing: 0, verticalAngle: 50, horizontalAngle: 50, verticalAngles: angles, horizontalAngles: angles, boost: false, eco: false, customFan: true, supportsBoost: true, supportsEco: true, minTemperature: 16, maxTemperature: 30, modes: [Choice(value: 1, name: "Auto"), Choice(value: 2, name: "Cool"), Choice(value: 3, name: "Dry"), Choice(value: 4, name: "Heat"), Choice(value: 5, name: "Fan")], fans: [], swings: [Choice(value: 0, name: "Off"), Choice(value: 12, name: "Vertical")])
    }
}
