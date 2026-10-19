import XCTest

final class ScreenTests: XCTestCase {
    func testListControlsAndVentSelection() {
        let app = XCUIApplication(); app.launchArguments = ["--demo"]; app.launch()
        XCTAssertTrue(app.buttons["device-1"].waitForExistence(timeout: 10))
        screenshot("Air conditioners")
        app.buttons["device-1"].tap()
        XCTAssertTrue(app.buttons["device-power"].firstMatch.waitForExistence(timeout: 5))
        app.buttons["Increase temperature"].tap()
        XCTAssertEqual(app.staticTexts["target-temperature"].label, "24")
        app.switches["Boost"].tap()
        app.switches["Automatic fan"].tap()
        XCTAssertEqual(app.switches["Boost"].value as? String, "0")
        XCTAssertEqual(app.switches["Automatic fan"].value as? String, "1")
        screenshot("Controls")
        XCTAssertTrue(app.buttons["Horizontal position 5"].isHittable)
        XCTAssertLessThanOrEqual(app.buttons["Horizontal position 5"].frame.maxY, app.frame.maxY - 4)
        app.buttons["Vertical position 2"].tap()
        XCTAssertTrue(app.buttons["Vertical position 2"].isSelected)
        screenshot("Vent positions")
    }
    func testLoginScreen() {
        let app = XCUIApplication(); app.launch()
        XCTAssertTrue(app.textFields["account"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["Sign in"].isEnabled)
        XCTAssertFalse(app.alerts.firstMatch.exists)
    }
    private func screenshot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot()); attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
}
