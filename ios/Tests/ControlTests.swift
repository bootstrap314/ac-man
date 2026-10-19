import XCTest
import Security
import AcManCore
@testable import AcManIOS

final class ControlTests: XCTestCase {
    func testSandboxKeychainRoundTrip() {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: "ac-man-test-" + UUID().uuidString,
            kSecAttrAccount as String: "sample"]
        var add = query; add[kSecValueData as String] = Data("sample".utf8)
        XCTAssertEqual(SecItemAdd(add as CFDictionary, nil), errSecSuccess)
        defer { SecItemDelete(query as CFDictionary) }
        var read = query; read[kSecReturnData as String] = true
        var result: CFTypeRef?
        XCTAssertEqual(SecItemCopyMatching(read as CFDictionary, &result), errSecSuccess)
        XCTAssertEqual(result as? Data, Data("sample".utf8))
    }
    func testExclusivePresets() {
        for preset in [FanPreset.automatic, .boost, .eco] {
            let change = SettingsChange.preset(preset, enabled: true, manual: 45, supportsBoost: true, supportsEco: true)
            XCTAssertEqual(change.fan, preset == .automatic ? 102 : 45)
            XCTAssertEqual(change.boost, preset == .boost)
            XCTAssertEqual(change.eco, preset == .eco)
        }
    }
    func testDisableDoesNotChangeOtherPresets() {
        let change = SettingsChange.preset(.boost, enabled: false, manual: 45, supportsBoost: true, supportsEco: true)
        XCTAssertEqual(change.boost, false)
        XCTAssertNil(change.fan)
        XCTAssertNil(change.eco)
    }
    func testRapidChangesMergeLatestValueAndPreserveOtherFields() {
        var first = SettingsChange.empty; first.temperature = 24; first.power = true
        var next = SettingsChange.empty; next.temperature = 26
        XCTAssertEqual(first.merging(next).temperature, 26)
        XCTAssertEqual(first.merging(next).power, true)
    }
    func testSpecialFanValuesNeverBecomePercentages() {
        XCTAssertEqual(fanPercentage(101), 50)
        XCTAssertEqual(fanPercentage(102), 50)
        XCTAssertEqual(fanPercentage(100), 100)
    }
    @MainActor func testOptimisticChangesAndQueueCompletion() async throws {
        let model = AppModel(demo: true); await model.start()
        var p = SettingsChange.empty; p.temperature = 25
        model.send("sample-1", p, debounce: true)
        p.temperature = 26; model.send("sample-1", p, debounce: true)
        XCTAssertEqual(model.device("sample-1")?.temperature, 26)
        try await Task.sleep(for: .seconds(1))
        XCTAssertTrue(model.pending.isEmpty)
        XCTAssertEqual(model.device("sample-1")?.temperature, 26)
    }
}
