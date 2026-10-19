import Foundation

/// Keeps an unconfirmed choice distinct from the device's reported position.
struct VentSelectionMemory {
    enum Axis: Hashable { case vertical, horizontal }
    private var choices: [String: [Axis: Int32]] = [:]
    mutating func select(device: String, axis: Axis, value: Int32?) {
        guard let value, [1, 25, 50, 75, 100].contains(value) else { return }
        choices[device, default: [:]][axis] = value
    }
    mutating func display(device: String, axis: Axis, reported: Int32?, pending: Int32?) -> (value: Int32?, confirmed: Bool) {
        if let pending {
            select(device: device, axis: axis, value: pending)
            return (choices[device]?[axis], false)
        }
        if let reported, [1, 25, 50, 75, 100].contains(reported) {
            select(device: device, axis: axis, value: reported)
            return (reported, true)
        }
        return (choices[device]?[axis], false)
    }
}
