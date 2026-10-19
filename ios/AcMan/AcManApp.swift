import SwiftUI
import AcManCore

@main struct AcManApp: App {
    @StateObject private var model = AppModel(demo: {
        #if DEBUG
        return ProcessInfo.processInfo.arguments.contains("--demo")
        #else
        return false
        #endif
    }())
    var body: some Scene { WindowGroup { RootView().environmentObject(model) } }
}
struct GlassBackground: View {
    @Environment(\.colorScheme) private var scheme
    var body: some View {
        LinearGradient(colors: scheme == .dark ? [Color(red: 0.08, green: 0.16, blue: 0.24), Color(red: 0.04, green: 0.07, blue: 0.12)] : [Color(red: 0.87, green: 0.94, blue: 1), Color(red: 0.97, green: 0.98, blue: 1)], startPoint: .topLeading, endPoint: .bottomTrailing).ignoresSafeArea()
    }
}
struct RootView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.scenePhase) var phase
    var body: some View {
        Group { if model.snapshot.signedIn { DeviceList() } else { LoginView() } }
            .task {
                await model.start()
                while !Task.isCancelled {
                    do { try await Task.sleep(for: .seconds(30)) } catch { return }
                    if phase == .active { await model.refresh() }
                }
            }
            .onChange(of: phase) { _, value in if value == .active { Task { await model.refresh() } } }
            .alert("ac-man", isPresented: Binding(get: { model.message != nil }, set: { if !$0 { model.message = nil } })) { Button("OK") { model.message = nil } } message: { Text(model.message ?? "") }
    }
}
struct LoginView: View {
    @EnvironmentObject var model: AppModel
    @State private var account = ""
    @State private var password = ""
    var body: some View {
        ZStack {
            GlassBackground()
            VStack(spacing: 24) {
                Image(systemName: "air.conditioner.horizontal.fill").font(.system(size: 54)).foregroundStyle(.cyan)
                Text("ac-man").font(.largeTitle.bold())
                Text("Your comfort, wherever you are.").foregroundStyle(.secondary)
                VStack(spacing: 16) {
                    TextField("SmartHome account", text: $account).textContentType(.username).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.emailAddress).accessibilityIdentifier("account")
                    Divider()
                    SecureField("Password", text: $password).textContentType(.password).accessibilityIdentifier("password")
                }.padding(20).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 22))
                Button { Task { await model.login(account: account.trimmingCharacters(in: .whitespacesAndNewlines), password: password); password = "" } } label: {
                    Group { if model.authenticating { ProgressView() } else { Text("Sign in").fontWeight(.semibold) } }.frame(maxWidth: .infinity).padding(10)
                }.buttonStyle(.borderedProminent).disabled(account.isEmpty || password.isEmpty || model.authenticating)
                Text("Sign in with your Midea SmartHome account.").font(.footnote).foregroundStyle(.secondary)
            }.padding(28).frame(maxWidth: 500)
        }
    }
}
struct DeviceList: View {
    @EnvironmentObject var model: AppModel
    @State private var settings = false
    var body: some View {
        NavigationStack {
            ZStack {
                GlassBackground()
                ScrollView {
                    LazyVStack(spacing: 16) {
                        ForEach(model.snapshot.devices, id: \.id) { raw in
                            if let d = model.device(raw.id) {
                                NavigationLink(value: d.id) {
                                    HStack(spacing: 16) {
                                        Image(systemName: "air.conditioner.horizontal").font(.system(size: 30)).foregroundStyle(.cyan)
                                        VStack(alignment: .leading, spacing: 7) {
                                            Text(d.label).font(.title3.weight(.semibold)).foregroundStyle(.primary)
                                            Text("Currently \(temperatureText(d.indoor))°C").foregroundStyle(.secondary)
                                            Text(d.online ? (d.power ? "\(d.modes.first(where: { $0.value == d.mode })?.name ?? "On") · \(temperatureText(d.temperature))°C" : "Off") : "Offline").font(.caption).foregroundStyle(.secondary)
                                        }
                                        Spacer(minLength: 0)
                                        Image(systemName: "power").font(.title3.bold()).foregroundStyle(d.power ? .green : .red).frame(width: 40, height: 40).background((d.power ? Color.green : .red).opacity(0.12), in: Circle())
                                        Image(systemName: "chevron.right").font(.caption.bold()).foregroundStyle(.secondary)
                                    }.padding(20).background(.regularMaterial, in: RoundedRectangle(cornerRadius: 24))
                                }.buttonStyle(.plain).accessibilityIdentifier("device-\(d.localId)")
                            }
                        }
                        if model.snapshot.devices.isEmpty { ContentUnavailableView("No air conditioners", systemImage: "air.conditioner.horizontal", description: Text("Devices linked to your SmartHome account will appear here.")) }
                    }.padding(20)
                }.refreshable { await model.refresh() }
            }.navigationTitle("Air conditioners")
                .toolbar { Button { settings = true } label: { Image(systemName: "gearshape") }.accessibilityLabel("Settings") }
                .navigationDestination(for: String.self) { DeviceControl(id: $0) }
                .confirmationDialog("Account", isPresented: $settings) { Button("Sign out", role: .destructive) { Task { await model.logout() } } }
        }.tint(.primary)
    }
}
struct DeviceControl: View {
    let id: String
    var body: some View {
        GeometryReader { geometry in
            DeviceControlContent(id: id, scale: min(1, max(0.72, geometry.size.height / 800)))
        }
    }
}
struct DeviceControlContent: View {
    let id: String
    let scale: CGFloat
    @EnvironmentObject var model: AppModel
    @State private var renaming = false
    @State private var name = ""
    @State private var fanDraft: Double?
    var body: some View {
        ZStack {
            GlassBackground()
            if let d = model.device(id) {
                ScrollView {
                    VStack(spacing: (scale < 0.85 ? 6 : 12) * scale) {
                        VStack(spacing: 8 * scale) {
                            Text(d.label).font(.system(size: 30 * scale, weight: .bold)).multilineTextAlignment(.center)
                                .overlay(alignment: .topTrailing) { Button { name = d.label; renaming = true } label: { Image(systemName: "pencil").font(.caption) }.offset(x: 23, y: -4).accessibilityLabel("Rename device") }
                            Text("Currently \(temperatureText(d.indoor))°C").font(.system(size: 19 * scale)).foregroundStyle(.secondary)
                            Button { var p = SettingsChange.empty; p.power = !d.power; model.send(id, p) } label: {
                                Image(systemName: "power").font(.system(size: 27 * scale, weight: .medium)).frame(width: 60 * scale, height: 60 * scale).foregroundStyle(d.power ? .green : .red).background((d.power ? Color.green : .red).opacity(0.16), in: Circle()).overlay(Circle().strokeBorder((d.power ? Color.green : .red).opacity(0.5)))
                            }.buttonStyle(.plain).padding(.top, 5 * scale).accessibilityLabel(d.power ? "Turn off" : "Turn on").accessibilityIdentifier("device-power")
                        }.padding(.horizontal, 24)
                        VStack(spacing: 8 * scale) {
                            Text("TARGET TEMPERATURE").font(.caption.weight(.semibold)).tracking(2).foregroundStyle(.secondary)
                            HStack {
                                temperatureButton("minus", color: .blue, delta: -1, device: d)
                                Spacer()
                                Text(temperatureText(d.temperature)).font(.system(size: 64 * scale, weight: .light, design: .rounded)).monospacedDigit().overlay(alignment: .topTrailing) { Text("°").font(.system(size: 32 * scale, weight: .light)).offset(x: 17, y: 5).accessibilityHidden(true) }.accessibilityIdentifier("target-temperature")
                                Spacer()
                                temperatureButton("plus", color: .red, delta: 1, device: d)
                            }
                        }.padding(.top, 10 * scale)
                        Picker("Mode", selection: Binding(get: { d.mode }, set: { var p = SettingsChange.empty; p.mode = $0; model.send(id, p) })) { ForEach(d.modes, id: \.value) { Text($0.name).tag($0.value) } }.pickerStyle(.segmented)
                        VStack(spacing: 8 * scale) {
                            HStack { Text("Fan speed"); Spacer(); Text("\(Int(fanDraft ?? Double(model.manualFan(d))))%").foregroundStyle(.secondary).monospacedDigit() }
                            Slider(value: Binding(get: { fanDraft ?? Double(model.manualFan(d)) }, set: { fanDraft = $0 }), in: 1...100, step: 1, onEditingChanged: { editing in
                                if !editing, let value = fanDraft { var p = SettingsChange.empty; p.fan = Int32(value); model.send(id, p); fanDraft = nil }
                            }).disabled(d.fan > 100 || d.boost || !d.customFan).accessibilityLabel("Fan speed")
                            if !d.customFan { Picker("Fan speed", selection: Binding(get: { d.fan }, set: { var p = SettingsChange.empty; p.fan = $0; model.send(id, p) })) { ForEach(d.fans.filter { (1...100).contains($0.value) }, id: \.value) { Text($0.name).tag($0.value) } }.disabled(d.fan > 100 || d.boost) }
                            preset("Automatic fan", .automatic, enabled: d.fan > 100, device: d)
                            if d.supportsBoost { preset("Boost", .boost, enabled: d.boost, device: d) }
                            if d.supportsEco { preset("Eco", .eco, enabled: d.eco, device: d) }
                        }
                        Divider()
                        if !d.swings.isEmpty { HStack { Text("Swing"); Spacer(); Picker("Swing", selection: Binding(get: { d.swing }, set: { var p = SettingsChange.empty; p.swing = $0; model.send(id, p) })) { ForEach(d.swings, id: \.value) { Text($0.name).tag($0.value) } } } }
                        vents(d, vertical: true)
                        vents(d, vertical: false)
                        if !d.online || model.errors[id] != nil { Label(model.errors[id] ?? "This air conditioner is offline.", systemImage: "wifi.exclamationmark").font(.footnote).foregroundStyle(.orange) }
                    }.padding(.horizontal, 24).padding(.vertical, 10 * scale).frame(maxWidth: 500).frame(maxWidth: .infinity).disabled(!d.online)
                }
            }
        }.navigationBarTitleDisplayMode(.inline)
            .alert("Rename device", isPresented: $renaming) { TextField("Name", text: $name); Button("Cancel", role: .cancel) {}; Button("Save") { Task { await model.rename(id, to: name) } } }
    }
    private func temperatureButton(_ symbol: String, color: Color, delta: Double, device d: DeviceSnapshot) -> some View {
        Button { var p = SettingsChange.empty; p.temperature = min(d.maxTemperature, max(d.minTemperature, (d.temperature ?? 23) + delta)); model.send(id, p, debounce: true) } label: {
            Image(systemName: symbol).font(.system(size: 24 * scale, weight: .medium)).foregroundStyle(color).frame(width: 54 * scale, height: 54 * scale).background(color.opacity(0.13), in: Circle()).overlay(Circle().strokeBorder(color.opacity(0.35)))
        }.buttonStyle(.plain).accessibilityLabel(delta > 0 ? "Increase temperature" : "Decrease temperature")
    }
    private func preset(_ title: String, _ preset: FanPreset, enabled: Bool, device: DeviceSnapshot) -> some View {
        Toggle(title, isOn: Binding(get: { enabled }, set: { model.setPreset(preset, enabled: $0, device: device) })).tint(.cyan).accessibilityIdentifier(title)
    }
    @ViewBuilder private func vents(_ d: DeviceSnapshot, vertical: Bool) -> some View {
        let choices = (vertical ? d.verticalAngles : d.horizontalAngles).filter { [1,25,50,75,100].contains($0.value) }
        if !choices.isEmpty {
            VStack(alignment: .leading, spacing: 6 * scale) {
                Text(vertical ? "Vertical position" : "Horizontal position").font(.subheadline).foregroundStyle(.secondary)
                HStack(spacing: 8) {
                    ForEach(Array(choices.enumerated()), id: \.element.value) { index, choice in
                        let selected = model.angle(d, vertical: vertical) == choice.value
                        Button { var p = SettingsChange.empty; if vertical { p.verticalAngle = choice.value } else { p.horizontalAngle = choice.value }; model.send(id, p) } label: {
                            VStack(spacing: 3 * scale) {
                                Image(systemName: vertical ? "arrow.up" : "arrow.left").rotationEffect(.degrees(Double(index) * 45)).font(.system(size: 18 * scale, weight: .medium))
                                Text("\(index + 1)").font(.caption)
                            }.frame(maxWidth: .infinity).padding(.vertical, 6 * scale).foregroundStyle(selected ? Color.cyan : .secondary).background(selected ? Color.cyan.opacity(0.16) : Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 13)).overlay(RoundedRectangle(cornerRadius: 13).strokeBorder(selected ? Color.cyan.opacity(0.6) : .clear))
                        }.buttonStyle(.plain).accessibilityLabel("\(vertical ? "Vertical" : "Horizontal") position \(index + 1)").accessibilityAddTraits(selected ? .isSelected : [])
                    }
                }
            }
        }
    }
}
