import Foundation
import Combine
import AcManCore

protocol AppController {
    func restoreLogin() async throws -> AppSnapshot
    func login(account: String, password: String, remember: Bool) async throws -> AppSnapshot
    func refresh() async throws -> AppSnapshot
    func logout() async throws -> AppSnapshot
    func rename(selector: String, label: String) async throws -> AppSnapshot
    func control(selector: String, change: SettingsChange) async throws -> AppSnapshot
    func snapshot() async throws -> AppSnapshot
}
extension AcController: AppController {}

@MainActor final class AppModel: ObservableObject {
    @Published private(set) var snapshot = AppSnapshot(signedIn: false, devices: [], route: "")
    @Published private(set) var pending: [String: SettingsChange] = [:]
    @Published private(set) var errors: [String: String] = [:]
    @Published var message: String?
    @Published private(set) var authenticating = false
    @Published private(set) var refreshing = false
    private var core: (any AppController)?
    @Published private(set) var restoring = false
    @Published private(set) var restored = false
    @Published private(set) var restoreError: String?
    private var queued: [String: SettingsChange] = [:]
    private var workers: [String: Task<Void, Never>] = [:]
    private var lastManual: [String: Int32] = [:]
    private var angles: [String: SettingsChange] = [:]
    let demo: Bool

    init(demo: Bool = false, controller: (any AppController)? = nil) {
        self.demo = demo; self.core = controller
    }
    func start() async {
        guard !restored, !restoring else { return }
        restoring = true
        restoreError = nil
        defer { restoring = false }
        if demo { snapshot = DemoData.snapshot; restored = true; return }
        do {
            if core == nil {
                let directory = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true).appendingPathComponent("ac-man")
                core = try AcController(dataDirectory: directory.path)
            }
            snapshot = try await core!.restoreLogin()
            restored = true
            await refresh()
        } catch {
            // A network or locked-Keychain failure is not a missing saved login.
            // Keep the credentials and retry on foreground, timer, or user request.
            restoreError = describe(error)
        }
    }
    func showLogin() {
        guard !restoring else { return }
        restored = true
        restoreError = nil
    }
    func resume() async {
        if restored { await refresh() } else { await start() }
    }
    func login(account: String, password: String) async {
        guard let core, !authenticating else { return }
        authenticating = true; defer { authenticating = false }
        do {
            snapshot = try await core.login(account: account, password: password, remember: true)
            message = nil; await refresh()
        } catch { message = describe(error) }
    }
    func refresh() async {
        guard let core, snapshot.signedIn, !refreshing, pending.isEmpty else { return }
        refreshing = true; defer { refreshing = false }
        do { snapshot = try await core.refresh() } catch { message = describe(error) }
    }
    func logout() async {
        guard pending.isEmpty else { message = "Wait for the current changes before signing out."; return }
        do {
            if let core { snapshot = try await core.logout() }
            else { snapshot = AppSnapshot(signedIn: false, devices: [], route: "") }
            errors = [:]; angles = [:]; lastManual = [:]
        } catch { message = describe(error) }
    }
    func rename(_ id: String, to name: String) async {
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        do {
            if let core { snapshot = try await core.rename(selector: id, label: name) }
            else if demo, let index = snapshot.devices.firstIndex(where: { $0.id == id }) { snapshot.devices[index].label = name }
        } catch { message = describe(error) }
    }
    func device(_ id: String) -> DeviceSnapshot? {
        guard var d = snapshot.devices.first(where: { $0.id == id }) else { return nil }
        let p = pending[id] ?? .empty
        d.power = p.power ?? d.power; d.temperature = p.temperature ?? d.temperature
        d.mode = p.mode ?? d.mode; d.fan = p.fan ?? d.fan; d.swing = p.swing ?? d.swing
        d.boost = p.boost ?? d.boost; d.eco = p.eco ?? d.eco
        d.verticalAngle = p.verticalAngle ?? d.verticalAngle
        d.horizontalAngle = p.horizontalAngle ?? d.horizontalAngle
        return d
    }
    func angle(_ d: DeviceSnapshot, vertical: Bool) -> Int32? {
        let raw = vertical ? d.verticalAngle : d.horizontalAngle
        if let raw, [1,25,50,75,100].contains(raw) { return raw }
        return vertical ? angles[d.id]?.verticalAngle : angles[d.id]?.horizontalAngle
    }
    func manualFan(_ d: DeviceSnapshot) -> Int32 {
        let value = lastManual[d.id] ?? fanPercentage(d.fan)
        if d.customFan { return value }
        return d.fans.map(\.value).filter { (1...100).contains($0) }.min(by: { abs($0-value) < abs($1-value) }) ?? value
    }
    func setPreset(_ preset: FanPreset, enabled: Bool, device d: DeviceSnapshot) {
        send(d.id, .preset(preset, enabled: enabled, manual: manualFan(d), supportsBoost: d.supportsBoost, supportsEco: d.supportsEco))
    }
    func send(_ id: String, _ change: SettingsChange, debounce: Bool = false) {
        guard let d = device(id), d.online, snapshot.signedIn else { return }
        if let fan = change.fan, (1...100).contains(fan) { lastManual[id] = fan }
        angles[id] = (angles[id] ?? .empty).merging(change)
        queued[id] = (queued[id] ?? .empty).merging(change)
        pending[id] = (pending[id] ?? .empty).merging(change)
        errors[id] = nil
        guard workers[id] == nil else { return }
        workers[id] = Task {
            while queued[id] != nil {
                // Coalesce rapid taps while keeping controls interactive.
                try? await Task.sleep(for: .milliseconds(debounce ? 450 : 120))
                guard let next = queued.removeValue(forKey: id) else { break }
                if demo {
                    if let index = snapshot.devices.firstIndex(where: { $0.id == id }), let value = device(id) { snapshot.devices[index] = value }
                } else if let core {
                    do { snapshot = try await core.control(selector: id, change: next) }
                    catch {
                        errors[id] = describe(error)
                        if let current = try? await core.snapshot() { snapshot = current }
                    }
                }
            }
            pending[id] = nil; workers[id] = nil
        }
    }
    private func describe(_ error: Error) -> String {
        switch error {
        case AcError.problem(let message): return message
        case AcError.cloud(let code, let message): return "SmartHome \(code): \(message)"
        default: return "Unable to complete the request. Please try again."
        }
    }
}
