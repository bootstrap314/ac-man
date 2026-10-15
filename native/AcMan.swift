import Cocoa
import AcManCore

// The generated async API runs the cloud work on Rust's Tokio runtime.
// AppKit views remain on the main actor and are created only once.
@MainActor
final class ControlWindow: NSWindow {
    override func sendEvent(_ event: NSEvent) {
        if event.type == .leftMouseDown {
            let point = event.locationInWindow
            // The native title bar is outside the content layout rectangle.
            // Leave the traffic-light controls to AppKit.
            if point.y >= contentLayoutRect.maxY && point.x >= 80 {
                performDrag(with: event)
                return
            }
        }
        super.sendEvent(event)
    }
}

@MainActor
final class FanSlider: NSSlider {
    private(set) var isDragging = false
    var onCommit: (() -> Void)?
    override func mouseDown(with event: NSEvent) {
        isDragging = true
        super.mouseDown(with: event)
        isDragging = false
        onCommit?()
    }
}

@MainActor
final class App: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private var core: AcController?
    private var snapshot = AppSnapshot(signedIn: false, devices: [], route: "Internet")
    private var statusItem: NSStatusItem!
    private let menu = NSMenu()
    private var timer: Timer?
    private var refreshing = false
    private var authenticating = false
    private var pending: [String: SettingsChange] = [:]
    private var operationErrors: [String: String] = [:]
    private var selectedID: String?
    private var manualSpeeds: [String: Int] = [:]
    private var fanSteps: [Int] = []

    private var loginWindow: NSWindow?
    private let email = NSTextField()
    private let password = NSSecureTextField()
    private let remember = NSButton(checkboxWithTitle: "Remember in macOS Keychain", target: nil, action: nil)
    private let loginMessage = NSTextField(wrappingLabelWithString: "")
    private let loginButton = NSButton(title: "Sign in", target: nil, action: nil)

    private var deviceWindow: NSWindow?
    private var deviceContents: NSStackView?
    private let heading = NSTextField(labelWithString: "Air conditioner")
    private let identity = NSTextField(labelWithString: "")
    private let roomStatus = NSTextField(labelWithString: "Waiting for status")
    private let power = NSButton(title: "Turn on", target: nil, action: nil)
    private let targetReading = NSTextField(labelWithString: "—°")
    private let cooler = NSButton(title: "−", target: nil, action: nil)
    private let warmer = NSButton(title: "+", target: nil, action: nil)
    private let mode = NSPopUpButton()
    private let modeSegments = NSSegmentedControl()
    private var modeRow: NSStackView?
    private var modeChoices: [Choice] = []
    private let fan = FanSlider(value: 50, minValue: 1, maxValue: 100, target: nil, action: nil)
    private let fanPercent = NSTextField(labelWithString: "—")
    private let autoFan = NSButton(checkboxWithTitle: "Auto", target: nil, action: nil)
    private let boost = NSButton(checkboxWithTitle: "Boost", target: nil, action: nil)
    private let fanNote = NSTextField(wrappingLabelWithString: "")
    private let swing = NSPopUpButton()
    private let verticalAngle = NSPopUpButton()
    private let horizontalAngle = NSPopUpButton()
    private var verticalAngleRow: NSStackView?
    private var horizontalAngleRow: NSStackView?
    private let footer = NSTextField(wrappingLabelWithString: "")
    private let spinner = NSProgressIndicator()
    private let retry = NSButton(title: "Refresh", target: nil, action: nil)
    private let rename = NSButton(title: "Rename room…", target: nil, action: nil)

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        installKeyboardMenu()
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        statusItem.button?.image = NSImage(systemSymbolName: "air.conditioner.horizontal", accessibilityDescription: "Air conditioners")
            ?? NSImage(systemSymbolName: "snowflake", accessibilityDescription: "Air conditioners")
        statusItem.button?.toolTip = "ac-man"
        menu.autoenablesItems = false
        menu.delegate = self
        statusItem.menu = menu
        rebuildMenu()
        Task {
            do {
                let directory = ProcessInfo.processInfo.environment["AC_MAN_DATA_DIR"]
                    ?? ProcessInfo.processInfo.environment["AC_CONTROL_DATA_DIR"]
                    ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/Midea Control").path
                core = try await Task.detached { try AcController(dataDirectory: directory) }.value
                guard let core else { return }
                accept(try await core.snapshot())
                authenticating = true
                accept(try await core.restoreLogin())
                authenticating = false
                if snapshot.signedIn { refreshAll() } else { showLogin(nil) }
            } catch {
                authenticating = false
                showLogin(nil)
                loginMessage.stringValue = errorMessage(error)
            }
        }
        timer = Timer.scheduledTimer(withTimeInterval: 45, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self, self.snapshot.signedIn, self.pending.isEmpty else { return }
                self.refreshAll()
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) { timer?.invalidate() }

    private func installKeyboardMenu() {
        let main = NSMenu()
        let appItem = NSMenuItem()
        let appMenu = NSMenu()
        let quit = NSMenuItem(title: "Quit ac-man", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appMenu.addItem(quit); appItem.submenu = appMenu; main.addItem(appItem)
        let edit = NSMenuItem(); edit.title = "Edit"
        let editMenu = NSMenu(title: "Edit")
        for (title, key, action) in [("Cut", "x", "cut:"), ("Copy", "c", "copy:"), ("Paste", "v", "paste:"), ("Select All", "a", "selectAll:")] {
            editMenu.addItem(NSMenuItem(title: title, action: Selector(action), keyEquivalent: key))
        }
        edit.submenu = editMenu; main.addItem(edit); NSApp.mainMenu = main
    }

    func menuNeedsUpdate(_ menu: NSMenu) { rebuildMenu() }

    private func item(_ title: String, _ action: Selector?, value: String? = nil) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self; item.representedObject = value; item.isEnabled = action != nil
        return item
    }

    private func rebuildMenu() {
        // Only rebuild when opening the dropdown, never while a request completes.
        menu.removeAllItems()
        menu.addItem(item("Air conditioners", nil))
        if snapshot.devices.isEmpty {
            menu.addItem(item(snapshot.signedIn ? "No air conditioners in this account" : "Sign in to discover your rooms", nil))
        }
        for device in snapshot.devices {
            let state: String
            if pending[device.id] != nil { state = "Updating…" }
            else if !snapshot.signedIn { state = "Sign in to control" }
            else if !device.error.isEmpty { state = "Connection issue" }
            else if device.online { state = device.power ? "On · \(number(device.temperature))°C" : "Off" }
            else { state = "Awaiting status" }
            menu.addItem(item("\(device.label)    \(state)", #selector(openDevice(_:)), value: device.id))
        }
        menu.addItem(.separator())
        let refresh = item(refreshing ? "Refreshing…" : "Refresh status", #selector(refreshAll))
        refresh.isEnabled = snapshot.signedIn && !refreshing; menu.addItem(refresh)
        if snapshot.signedIn {
            let logout = item("Sign out & forget saved login…", #selector(signOut))
            logout.isEnabled = pending.isEmpty && !authenticating; menu.addItem(logout)
        } else {
            let login = item("Sign in to SmartHome…", #selector(showLogin(_:)))
            login.isEnabled = core != nil && !authenticating; menu.addItem(login)
        }
        menu.addItem(.separator()); menu.addItem(item("Quit ac-man", #selector(quit)))
    }

    private func accept(_ value: AppSnapshot) {
        snapshot = value
        updateDevice()
    }

    @objc private func refreshAll() {
        guard let core, snapshot.signedIn, !refreshing, !authenticating else { return }
        refreshing = true; updateDevice()
        Task {
            do {
                accept(try await core.refresh())
                operationErrors = operationErrors.filter { id, _ in pending[id] != nil }
            } catch {
                if let id = selectedID { operationErrors[id] = errorMessage(error) }
            }
            refreshing = false; updateDevice()
        }
    }

    private func window(_ title: String, width: CGFloat, height: CGFloat) -> NSWindow {
        let result = ControlWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: height), styleMask: [.titled, .closable], backing: .buffered, defer: false)
        result.title = title; result.isReleasedWhenClosed = false; result.center()
        return result
    }

    private func stack(in window: NSWindow, padding: CGFloat = 24, spacing: CGFloat = 14, topPadding: CGFloat? = nil) -> NSStackView {
        let stack = NSStackView(); stack.orientation = .vertical; stack.alignment = .leading; stack.spacing = spacing
        stack.translatesAutoresizingMaskIntoConstraints = false
        let content = (window.contentView as? ClimateBackgroundView)?.contentContainer ?? window.contentView!; content.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: content.leadingAnchor, constant: padding),
            stack.trailingAnchor.constraint(equalTo: content.trailingAnchor, constant: -padding),
            stack.topAnchor.constraint(equalTo: content.topAnchor, constant: topPadding ?? padding)
        ])
        return stack
    }

    private func full(_ view: NSView, in stack: NSStackView) {
        stack.addArrangedSubview(view); view.widthAnchor.constraint(equalTo: stack.widthAnchor).isActive = true
    }

    @discardableResult
    private func row(_ title: String, _ control: NSView, in stack: NSStackView) -> NSStackView {
        let label = NSTextField(labelWithString: title)
        label.widthAnchor.constraint(equalToConstant: 120).isActive = true
        let row = NSStackView(views: [label, control]); row.orientation = .horizontal; row.spacing = 12
        control.setContentHuggingPriority(.defaultLow, for: .horizontal)
        full(row, in: stack)
        return row
    }

    private func configure(_ control: NSControl, _ action: Selector) { control.target = self; control.action = action }

    @objc private func showLogin(_ sender: Any?) {
        if loginWindow == nil {
            let win = window("SmartHome sign-in", width: 430, height: 420); loginWindow = win
            let contents = stack(in: win)
            let title = NSTextField(labelWithString: "Your rooms. One click away.")
            title.font = .systemFont(ofSize: 20, weight: .semibold); full(title, in: contents)
            full(NSTextField(wrappingLabelWithString: "Sign in with your SmartHome account. Your air conditioners will appear automatically."), in: contents)
            email.placeholderString = "SmartHome email"; email.setAccessibilityLabel("SmartHome email")
            password.placeholderString = "Password"; password.setAccessibilityLabel("SmartHome password")
            full(email, in: contents); full(password, in: contents); contents.addArrangedSubview(remember)
            full(NSTextField(wrappingLabelWithString: "Choose Keychain to share your login with the CLI and restore it when you reopen the app."), in: contents)
            configure(loginButton, #selector(submitLogin)); loginButton.bezelStyle = .rounded; loginButton.keyEquivalent = "\r"
            contents.addArrangedSubview(loginButton)
            loginMessage.textColor = .secondaryLabelColor; full(loginMessage, in: contents)
            win.initialFirstResponder = email
        }
        loginButton.isEnabled = core != nil && !authenticating
        NSApp.activate(ignoringOtherApps: true); loginWindow?.makeKeyAndOrderFront(nil)
    }

    @objc private func submitLogin() {
        guard let core, !authenticating else { return }
        let account = email.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !account.isEmpty, !password.stringValue.isEmpty else {
            loginMessage.stringValue = "Enter your SmartHome email and password."; return
        }
        let secret = password.stringValue; password.stringValue = ""
        let save = remember.state == .on
        authenticating = true; loginButton.isEnabled = false; loginMessage.stringValue = "Signing in…"
        Task {
            do {
                accept(try await core.login(account: account, password: secret, remember: save))
                loginMessage.stringValue = ""; loginWindow?.orderOut(nil)
                authenticating = false; refreshAll()
            } catch { loginMessage.stringValue = errorMessage(error); authenticating = false }
            loginButton.isEnabled = true
        }
    }

    @objc private func signOut() {
        guard let core, !authenticating, pending.isEmpty else { return }
        let alert = NSAlert(); alert.messageText = "Sign out of SmartHome?"
        alert.informativeText = "This removes the shared app and CLI login from Keychain. Your room labels stay on this Mac."
        alert.addButton(withTitle: "Sign out"); alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        authenticating = true
        Task {
            do { accept(try await core.logout()); operationErrors.removeAll() }
            catch { showError(errorMessage(error)) }
            authenticating = false; updateDevice()
        }
    }

    @objc private func openDevice(_ sender: NSMenuItem) {
        selectedID = sender.representedObject as? String
        if deviceWindow == nil { buildDeviceWindow() }
        updateDevice()
        NSApp.activate(ignoringOtherApps: true); deviceWindow?.makeKeyAndOrderFront(nil)
    }

    private func buildDeviceWindow() {
        let win = window("Air conditioner", width: 380, height: 380); deviceWindow = win
        win.isOpaque = false; win.backgroundColor = .clear
        win.styleMask.insert(.fullSizeContentView)
        win.titleVisibility = .hidden
        win.titlebarAppearsTransparent = true
        win.isMovable = true
        win.isMovableByWindowBackground = true
        win.contentView = ClimateBackgroundView()
        let contents = stack(in: win, padding: 20, spacing: 12, topPadding: 44); deviceContents = contents
        heading.font = .systemFont(ofSize: 20, weight: .semibold)
        heading.lineBreakMode = .byTruncatingTail
        heading.setContentHuggingPriority(.defaultLow, for: .horizontal)
        configure(power, #selector(togglePower)); power.bezelStyle = .rounded
        power.contentTintColor = .systemBlue
        let header = NSStackView(views: [heading, power]); header.spacing = 12; full(header, in: contents)
        identity.font = .systemFont(ofSize: 11); identity.textColor = .secondaryLabelColor
        identity.lineBreakMode = .byTruncatingMiddle
        identity.setContentHuggingPriority(.defaultLow, for: .horizontal)
        configure(rename, #selector(renameRoom)); rename.bezelStyle = .rounded
        rename.title = "Rename…"; rename.setAccessibilityLabel("Rename room")
        let details = NSStackView(views: [identity, rename]); details.spacing = 8; full(details, in: contents)
        roomStatus.font = .systemFont(ofSize: 15, weight: .semibold)
        roomStatus.setContentHuggingPriority(.defaultLow, for: .horizontal)
        roomStatus.font = .systemFont(ofSize: 12)
        roomStatus.textColor = .secondaryLabelColor
        full(roomStatus, in: contents)
        let targetCaption = NSTextField(labelWithString: "TARGET TEMPERATURE")
        targetCaption.font = .systemFont(ofSize: 10, weight: .semibold)
        targetCaption.textColor = .secondaryLabelColor
        targetCaption.alignment = .center; full(targetCaption, in: contents)
        targetReading.font = .monospacedDigitSystemFont(ofSize: 52, weight: .light)
        targetReading.alignment = .center
        targetReading.setContentHuggingPriority(.defaultLow, for: .horizontal)
        targetReading.setAccessibilityLabel("Target temperature")
        for (button, action, label) in [(cooler, #selector(lowerTemperature), "Decrease temperature"), (warmer, #selector(raiseTemperature), "Increase temperature")] {
            configure(button, action); button.bezelStyle = .circular
            button.font = .systemFont(ofSize: 24, weight: .regular)
            button.contentTintColor = .systemBlue
            button.widthAnchor.constraint(equalToConstant: 40).isActive = true
            button.heightAnchor.constraint(equalToConstant: 40).isActive = true
            button.setAccessibilityLabel(label)
        }
        let temperatureRow = NSStackView(views: [cooler, targetReading, warmer]); temperatureRow.spacing = 12
        full(temperatureRow, in: contents)
        configure(mode, #selector(setMode)); modeRow = row("Mode", mode, in: contents)
        configure(modeSegments, #selector(setSegmentMode)); modeSegments.segmentStyle = .rounded
        modeSegments.trackingMode = .selectOne; modeSegments.setAccessibilityLabel("Operating mode")
        full(modeSegments, in: contents)
        fanPercent.alignment = .right; fanPercent.font = .monospacedDigitSystemFont(ofSize: 13, weight: .medium)
        fanPercent.widthAnchor.constraint(equalToConstant: 40).isActive = true
        configure(fan, #selector(previewFan)); fan.isContinuous = true; fan.setAccessibilityLabel("Fan speed percentage")
        fan.onCommit = { [weak self] in self?.commitFan() }
        let fanControl = NSStackView(views: [fan, fanPercent]); fanControl.spacing = 8
        row("Fan speed", fanControl, in: contents)
        configure(autoFan, #selector(setAutoFan)); configure(boost, #selector(setBoost))
        for button in [autoFan, boost] {
            button.setButtonType(.pushOnPushOff); button.bezelStyle = .rounded
            button.contentTintColor = .systemBlue
        }
        let fanOptions = NSStackView(views: [autoFan, boost]); fanOptions.spacing = 28; contents.addArrangedSubview(fanOptions)
        fanNote.font = .systemFont(ofSize: 11); fanNote.textColor = .secondaryLabelColor
        full(fanNote, in: contents); fanNote.heightAnchor.constraint(equalToConstant: 30).isActive = true
        configure(swing, #selector(setSwing)); row("Swing", swing, in: contents)
        configure(verticalAngle, #selector(setVerticalAngle))
        verticalAngle.setAccessibilityLabel("Vertical vent position")
        verticalAngleRow = row("Vertical vent", verticalAngle, in: contents)
        configure(horizontalAngle, #selector(setHorizontalAngle))
        horizontalAngle.setAccessibilityLabel("Horizontal vent position")
        horizontalAngleRow = row("Horizontal vent", horizontalAngle, in: contents)
        spinner.style = .spinning; spinner.controlSize = .small; spinner.isDisplayedWhenStopped = false
        spinner.widthAnchor.constraint(equalToConstant: 16).isActive = true
        footer.font = .systemFont(ofSize: 11); footer.textColor = .secondaryLabelColor
        footer.heightAnchor.constraint(equalToConstant: 42).isActive = true
        configure(retry, #selector(refreshAll)); retry.bezelStyle = .rounded
        let bottom = NSStackView(views: [spinner, footer, retry]); bottom.spacing = 10; full(bottom, in: contents)
        footer.setContentHuggingPriority(.defaultLow, for: .horizontal)
    }

    private var selected: DeviceSnapshot? { snapshot.devices.first { $0.id == selectedID } }

    private func updateDevice() {
        guard let win = deviceWindow, let d = selected else { return }
        win.title = d.label; heading.stringValue = d.label
        identity.stringValue = "Device \(d.localId)"
        let change = pending[d.id]
        let enabled = snapshot.signedIn && d.online && change == nil && !authenticating
        rename.isEnabled = change == nil
        power.isEnabled = enabled; modeSegments.isEnabled = enabled;
        let target = change?.temperature ?? d.temperature
        targetReading.stringValue = target.map { "\(number($0))°" } ?? "—°"
        targetReading.setAccessibilityValue(target.map { "\(number($0)) degrees Celsius" } ?? "Unknown")
        cooler.isEnabled = enabled && target != nil && target! > d.minTemperature
        warmer.isEnabled = enabled && target != nil && target! < d.maxTemperature; mode.isEnabled = enabled; swing.isEnabled = enabled
        verticalAngleRow?.isHidden = d.verticalAngles.isEmpty
        horizontalAngleRow?.isHidden = d.horizontalAngles.isEmpty
        verticalAngle.isEnabled = enabled && !d.verticalAngles.isEmpty
        horizontalAngle.isEnabled = enabled && !d.horizontalAngles.isEmpty
        retry.isEnabled = snapshot.signedIn && !refreshing
        if d.online {
            let powerOn = change?.power ?? d.power
            let boostOn = change?.boost ?? d.boost
            let speed = change?.fan ?? d.fan
            roomStatus.stringValue = "\(powerOn ? "On" : "Off") · Room \(number(d.indoor))°C"
            power.title = powerOn ? "Turn off" : "Turn on"
            updatePicker(mode, choices: d.modes, value: change?.mode ?? d.mode)
            if modeChoices != d.modes {
                modeChoices = d.modes; modeSegments.segmentCount = d.modes.count
                for (index, choice) in d.modes.enumerated() {
                    modeSegments.setLabel(choice.name, forSegment: index)
                }
            }
            modeSegments.selectedSegment = d.modes.firstIndex { $0.value == (change?.mode ?? d.mode) } ?? -1
            let compactModes = (1...4).contains(d.modes.count)
            modeRow?.isHidden = compactModes; modeSegments.isHidden = !compactModes
            updatePicker(swing, choices: d.swings, value: change?.swing ?? d.swing)
            updateAnglePicker(verticalAngle, choices: d.verticalAngles, value: change?.verticalAngle ?? d.verticalAngle)
            updateAnglePicker(horizontalAngle, choices: d.horizontalAngles, value: change?.horizontalAngle ?? d.horizontalAngle)
            fanSteps = d.customFan ? Array(1...100) : d.fans.map { Int($0.value) }.filter { (1...100).contains($0) }.sorted()
            if !fan.isDragging {
                if (1...100).contains(Int(speed)) && !boostOn { manualSpeeds[d.id] = Int(speed) }
                fan.minValue = Double(fanSteps.first ?? 1); fan.maxValue = Double(fanSteps.last ?? 100)
                fan.doubleValue = Double(speed == 102 ? (manualSpeeds[d.id] ?? 50) : Int(speed))
                fanPercent.stringValue = speed == 102 ? "Auto" : "\(speed)%"
                autoFan.state = speed == 102 ? .on : .off
                fan.setAccessibilityValueDescription(fanPercent.stringValue)
            }
            fan.isEnabled = enabled && !boostOn && !fanSteps.isEmpty
            autoFan.isEnabled = enabled && !boostOn && d.fans.contains { $0.value == 102 }
            boost.state = boostOn ? .on : .off; boost.isEnabled = enabled && d.supportsBoost
            fanNote.stringValue = boostOn ? "Boost controls the fan speed. Turn it off to adjust manually."
                : (d.customFan ? "Drag to preview. Release to apply." : "This model supports fixed fan speeds; the slider snaps to them.")
        } else {
            roomStatus.stringValue = "Status unavailable"
            power.title = "Power"
            fan.isEnabled = false; autoFan.isEnabled = false; boost.isEnabled = false
        }
        if change != nil { footer.stringValue = "Applying and checking your change…" }
        else if let error = operationErrors[d.id] { footer.stringValue = error }
        else if !snapshot.signedIn { footer.stringValue = "Sign in from the menu bar to control this AC." }
        else if refreshing { footer.stringValue = "Refreshing in the background…" }
        else if !d.error.isEmpty { footer.stringValue = d.error }
        else if d.updated > 0 { footer.stringValue = "Updated \(Date(timeIntervalSince1970: d.updated).formatted(date: .omitted, time: .shortened))" }
        else { footer.stringValue = "Waiting for the first status update." }
        if change != nil || refreshing { spinner.startAnimation(nil) } else { spinner.stopAnimation(nil) }
        fitDeviceWindow()
    }

    private func fitDeviceWindow() {
        guard let win = deviceWindow, let contents = deviceContents else { return }
        // Fit the actual rows without animating or moving the title bar during refreshes.
        win.contentView?.layoutSubtreeIfNeeded()
        let height = ceil(contents.fittingSize.height) + 64
        guard abs((win.contentView?.bounds.height ?? 0) - height) > 1 else { return }
        let top = win.frame.maxY
        win.setContentSize(NSSize(width: 380, height: height))
        win.setFrameOrigin(NSPoint(x: win.frame.minX, y: top - win.frame.height))
    }

    private func updatePicker(_ picker: NSPopUpButton, choices: [Choice], value: Int32) {
        // Updating a value must not replace the control or discard focus.
        let existing = picker.itemArray.map { Choice(value: Int32($0.tag), name: $0.title) }
        if existing != choices {
            picker.removeAllItems()
            for choice in choices { picker.addItem(withTitle: choice.name); picker.lastItem?.tag = Int(choice.value) }
        }
        picker.selectItem(withTag: Int(value))
    }

    private func updateAnglePicker(_ picker: NSPopUpButton, choices: [Choice], value: Int32?) {
        // A supported vent may not have reported its position yet; don't imply it is off.
        let available = value == nil ? [Choice(value: -1, name: "Unknown")] + choices : choices
        updatePicker(picker, choices: available, value: value ?? -1)
        picker.menu?.item(withTag: -1)?.isEnabled = false
    }

    private func change(power: Bool? = nil, temperature: Double? = nil, mode: Int32? = nil, fan: Int32? = nil, swing: Int32? = nil, boost: Bool? = nil, verticalAngle: Int32? = nil, horizontalAngle: Int32? = nil) {
        guard let core, let d = selected, snapshot.signedIn, d.online, pending[d.id] == nil else { return }
        let change = SettingsChange(power: power, temperature: temperature, mode: mode, fan: fan, swing: swing, boost: boost, verticalAngle: verticalAngle, horizontalAngle: horizontalAngle)
        pending[d.id] = change; operationErrors.removeValue(forKey: d.id); updateDevice()
        Task {
            do { accept(try await core.control(selector: d.id, change: change)) }
            catch {
                operationErrors[d.id] = errorMessage(error)
                if let current = try? await core.snapshot() { accept(current) }
            }
            pending.removeValue(forKey: d.id); updateDevice()
        }
    }

    @objc private func togglePower() { if let d = selected { change(power: !d.power) } }
    @objc private func lowerTemperature() { adjustTemperature(by: -0.5) }
    @objc private func raiseTemperature() { adjustTemperature(by: 0.5) }
    private func adjustTemperature(by step: Double) {
        guard let d = selected, let value = d.temperature else { return }
        change(temperature: min(d.maxTemperature, max(d.minTemperature, value + step)))
    }
    @objc private func setSegmentMode() {
        guard modeChoices.indices.contains(modeSegments.selectedSegment) else { return }
        change(mode: modeChoices[modeSegments.selectedSegment].value)
    }
    @objc private func setMode() { change(mode: Int32(mode.selectedTag())) }
    @objc private func setSwing() { change(swing: Int32(swing.selectedTag())) }
    @objc private func setVerticalAngle() { change(verticalAngle: Int32(verticalAngle.selectedTag())) }
    @objc private func setHorizontalAngle() { change(horizontalAngle: Int32(horizontalAngle.selectedTag())) }
    @objc private func setBoost() { change(boost: boost.state == .on) }
    @objc private func setAutoFan() { change(fan: autoFan.state == .on ? 102 : Int32(fanPercentage())) }
    private func fanPercentage() -> Int {
        let value = Int(fan.doubleValue.rounded())
        return fanSteps.min { abs($0 - value) < abs($1 - value) } ?? min(100, max(1, value))
    }
    @objc private func previewFan() {
        let value = fanPercentage(); fan.doubleValue = Double(value)
        fanPercent.stringValue = "\(value)%"; autoFan.state = .off
        fan.setAccessibilityValueDescription("\(value) percent")
        if !fan.isDragging { commitFan() }
    }
    private func commitFan() {
        guard fan.isEnabled, let d = selected else { return }
        let value = fanPercentage(); manualSpeeds[d.id] = value
        if d.fan != value { change(fan: Int32(value)) }
    }

    @objc private func renameRoom() {
        guard let core, let d = selected else { return }
        let alert = NSAlert(); alert.messageText = "Name this room"
        alert.informativeText = "This name is shared by the app and CLI on this Mac."
        let input = NSTextField(frame: NSRect(x: 0, y: 0, width: 280, height: 24)); input.stringValue = d.label
        alert.accessoryView = input; alert.addButton(withTitle: "Save"); alert.addButton(withTitle: "Cancel")
        alert.window.initialFirstResponder = input
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        let label = input.stringValue
        Task {
            do { accept(try await core.rename(selector: d.id, label: label)) }
            catch { showError(errorMessage(error)) }
        }
    }

    private func number(_ value: Double?) -> String {
        guard let value else { return "—" }
        return value == value.rounded() ? String(Int(value)) : String(format: "%.1f", value)
    }
    private func errorMessage(_ error: Error) -> String {
        switch error {
        case AcError.problem(let message): return message
        case AcError.cloud(let code, let message): return "SmartHome error \(code): \(message)"
        default: return "The controller could not complete the request. Please try again."
        }
    }
    private func showError(_ message: String) {
        let alert = NSAlert(); alert.messageText = "ac-man"; alert.informativeText = message
        NSApp.activate(ignoringOtherApps: true); alert.runModal()
    }
    @objc private func quit() { NSApp.terminate(nil) }
}

@main
struct AcMan {
    @MainActor static func main() {
        let application = NSApplication.shared
        let delegate = App(); application.delegate = delegate
        withExtendedLifetime(delegate) { application.run() }
    }
}
