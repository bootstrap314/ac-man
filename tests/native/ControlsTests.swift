import Cocoa

@MainActor
private final class SelectionRecorder: NSObject {
    var values: [Int32?] = []
    @objc func record(_ control: VentPositionControl) { values.append(control.selectedValue) }
}

@main
struct ControlsTests {
    @MainActor static func main() {
        _ = NSApplication.shared
        var checks = 0
        func check(_ condition: @autoclosure () -> Bool, _ message: String) {
            guard condition() else { fatalError(message) }
            checks += 1
        }
        for axis in [VentPositionControl.Axis.vertical, .horizontal] {
            let control = VentPositionControl(axis: axis)
            let recorder = SelectionRecorder()
            control.target = recorder; control.action = #selector(SelectionRecorder.record(_:))
            let supported: [Int32] = [0, 1, 25, 50, 75, 100]
            control.update(values: supported, selected: nil)
            let stack = control.subviews[0] as! NSStackView
            let buttons = stack.arrangedSubviews.compactMap { $0 as? NSButton }
            check(buttons.count == 6, "Each supported position needs a button")
            check(buttons.allSatisfy { $0.state == .off }, "Unknown position must not imply Off")
            for (button, value) in zip(buttons, supported) {
                button.performClick(nil)
                check(control.selectedValue == value, "Wrong protocol position sent")
                check(recorder.values.last! == value, "Selection action must carry the protocol value")
                check(buttons.filter { $0.state == .on }.count == 1, "Exactly one position must be selected")
            }
            control.update(values: supported, selected: 25)
            check(stack.arrangedSubviews[2] === buttons[2], "Refresh must preserve button identity and keyboard focus")
            control.update(values: supported, selected: 99)
            check(control.selectedValue == nil && buttons.allSatisfy { $0.state == .off }, "Unrecognized positions must remain unselected")
            control.isEnabled = false
            let before = recorder.values.count
            buttons[1].performClick(nil)
            check(recorder.values.count == before, "Disabled controls must not send actions")
            check(buttons.allSatisfy { !$0.isEnabled }, "Disabled state must reach every button")
            control.update(values: [0, 50], selected: 50)
            check(stack.arrangedSubviews.count == 2, "Unsupported positions must not be offered")
            check(stack.arrangedSubviews.compactMap { $0 as? NSButton }.allSatisfy { !$0.isEnabled }, "Rebuilding cannot enable offline controls")
            control.isEnabled = true
            check(stack.arrangedSubviews.compactMap { $0 as? NSButton }.allSatisfy { $0.isEnabled }, "Controls must recover after reconnect")
        }
        for code: Int32 in [101, 103, 127, -1] {
            check(FanSpeedDisplay.label(code, boost: false) == "—", "Reserved fan code cannot be a percentage")
            check(FanSpeedDisplay.sliderValue(code, lastManual: 45) == 45, "Unknown fan code must retain the manual slider value")
        }
        check(FanSpeedDisplay.label(102, boost: false) == "Auto", "Auto fan label")
        check(FanSpeedDisplay.label(101, boost: true) == "Boost", "Boost label takes priority")
        check(FanSpeedDisplay.label(100, boost: false) == "100%", "Maximum valid percentage")
        check(FanSpeedDisplay.sliderValue(101, lastManual: nil) == 50, "Unknown slider fallback")
        print("Native control tests passed: \(checks) checks")
    }
}
