import SwiftUI
import ServiceManagement
import AppKit

struct SettingsView: View {
    @ObservedObject private var state = AppState.shared
    @State private var apiKey: String = KeychainStore.shared.get("anthropic-api-key") ?? ""

    // Claude model — dynamic list fetched from the API, static fallback if unavailable
    private static let fallbackModels: [(id: String, label: String)] = [
        ("claude-sonnet-4-6",         "Claude Sonnet 4.6"),
        ("claude-sonnet-5-5",         "Claude Sonnet 5.5"),
        ("claude-opus-5-5",           "Claude Opus 5.5"),
        ("claude-haiku-4-5-20251001", "Claude Haiku 4.5"),
    ]
    private static let customModelTag = "__custom__"
    @State private var fetchedModels: [(id: String, label: String)] = []
    @State private var modelChoice: String = {
        let m = AppState.shared.claudeModel
        return SettingsView.fallbackModels.contains { $0.id == m } ? m : SettingsView.customModelTag
    }()
    @State private var customModel: String = {
        let m = AppState.shared.claudeModel
        return SettingsView.fallbackModels.contains { $0.id == m } ? "" : m
    }()
    private var displayModels: [(id: String, label: String)] {
        fetchedModels.isEmpty ? Self.fallbackModels : fetchedModels
    }
    @State private var launchAtStartup: Bool = (SMAppService.mainApp.status == .enabled)
    @State private var statusMessage: String = ""
    @State private var showDiff: Bool = false
    @State private var pendingHookJSON: String = ""
    @State private var hookNeedsUpdate: Bool = HookServer.hooksNeedUpdate()

    #if !APPSTORE
    @State private var showStatusLineDiff: Bool = false
    @State private var pendingStatusLineJSON: String = ""
    @State private var statusLinePendingInstall: Bool = true
    @State private var planTogglePending: Bool = false

    @State private var geminiHooksInstalled: Bool = HookServer.geminiHooksInstalled()
    @State private var showGeminiDiff: Bool = false
    @State private var pendingGeminiJSON: String = ""
    @State private var geminiPendingInstall: Bool = true

    @State private var agyHooksInstalled: Bool = HookServer.agyHooksInstalled()
    @State private var showAgyDiff: Bool = false
    @State private var pendingAgyJSON: String = ""
    @State private var agyPendingInstall: Bool = true

    @State private var codexHooksInstalled: Bool = HookServer.codexHooksInstalled()
    @State private var showCodexDiff: Bool = false
    @State private var pendingCodexJSON: String = ""
    @State private var codexPendingInstall: Bool = true

    @State private var copilotHooksInstalled: Bool = HookServer.copilotHooksInstalled()
    @State private var showCopilotDiff: Bool = false
    @State private var pendingCopilotJSON: String = ""
    @State private var copilotPendingInstall: Bool = true
    #endif

    // Multi-provider chat keys
    @State private var googleKey: String  = KeychainStore.shared.get("google-api-key") ?? ""
    @State private var openAIKey: String  = KeychainStore.shared.get("openai-api-key") ?? ""
    @State private var ollamaURL:    String = AppState.shared.ollamaServerURL
    @State private var lmstudioURL:  String = AppState.shared.lmstudioServerURL
    @State private var connectingOllama:    Bool = false
    @State private var connectingLMStudio:  Bool = false

    // Integration keys
    @State private var resendKey: String    = KeychainStore.shared.get("resend-api-key")  ?? ""
    @State private var resendFrom: String   = KeychainStore.shared.get("resend-from")     ?? ""
    @State private var n8nUrl: String       = KeychainStore.shared.get("n8n-url")         ?? ""
    @State private var n8nKey: String       = KeychainStore.shared.get("n8n-api-key")     ?? ""
    @State private var vercelToken: String  = KeychainStore.shared.get("vercel-token")    ?? ""
    @State private var githubToken: String  = KeychainStore.shared.get("github-token")    ?? ""
    @State private var stripeKey: String    = KeychainStore.shared.get("stripe-api-key")  ?? ""
    @State private var calcomKey: String    = KeychainStore.shared.get("calcom-api-key")  ?? ""
    @State private var notionKey: String    = KeychainStore.shared.get("notion-api-key")  ?? ""

    // Hotkey
    @State private var hotkeyFlags: UInt    = AppState.shared.hotkeyFlags
    @State private var hotkeyCode: UInt16   = AppState.shared.hotkeyCode

    // Vercel project filter
    @State private var vercelProjects: [String] = []
    @State private var loadingVercel: Bool = false

    // n8n workflow filter
    @State private var n8nWorkflows: [String] = []
    @State private var loadingN8n: Bool = false

    // Bindings in minutes for the absence field
    private var absenceMinutes: Binding<Double> {
        Binding(
            get: { state.absenceInterval / 60 },
            set: { state.absenceInterval = max(1, $0) * 60 }
        )
    }

    // Sidebar selection persisted across sessions
    @AppStorage("settingsSection") private var selectedSection: String = "general"

    private var appVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""
    }

    // MARK: - Body

    var body: some View {
        HStack(spacing: 0) {
            // Sidebar — 200 pt, sidebar visual effect background
            ZStack(alignment: .topLeading) {
                SidebarBackground()
                VStack(alignment: .leading, spacing: 0) {
                    // Header
                    HStack(alignment: .center, spacing: 10) {
                        Image(nsImage: NSApplication.shared.applicationIconImage)
                            .resizable()
                            .frame(width: 32, height: 32)
                        VStack(alignment: .leading, spacing: 1) {
                            Text("Coucou")
                                .font(.system(size: 13, weight: .semibold))
                            Text(appVersion)
                                .font(.system(size: 11))
                                .foregroundColor(.secondary)
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, 16)
                    .padding(.bottom, 10)
                    Divider()
                    List(selection: Binding(
                        get: { Optional(selectedSection) },
                        set: { if let v = $0 { selectedSection = v; statusMessage = "" } }
                    )) {
                        SettingsSidebarRow(title: L10n.t("settings.general"),      icon: "gearshape.fill",                    color: "#8E939C").tag("general")
                        SettingsSidebarRow(title: L10n.t("settings.activePills"), icon: "square.grid.2x2.fill",              color: "#F5A524").tag("activepills")
                        SettingsSidebarRow(title: L10n.t("settings.agents"),       icon: "terminal.fill",                     color: "#3B9EFF").tag("agents")
                        SettingsSidebarRow(title: L10n.t("settings.chat"),         icon: "bubble.left.and.bubble.right.fill", color: "#E07950").tag("chat")
                        SettingsSidebarRow(title: L10n.t("settings.integrations"), icon: "puzzlepiece.extension.fill",        color: "#7C5CFF").tag("integrations")
                    }
                    .listStyle(.sidebar)
                    .scrollContentBackground(.hidden)
                }
            }
            .frame(width: 200)

            Divider()

            // Detail panel
            VStack(alignment: .leading, spacing: 0) {
                Text(sectionTitle)
                    .font(.title2)
                    .fontWeight(.semibold)
                    .padding(.horizontal, 20)
                    .padding(.top, 20)
                    .padding(.bottom, 12)
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        sectionContent
                    }
                    .padding(.horizontal, 20)
                    .padding(.bottom, 16)
                }
                if !statusMessage.isEmpty {
                    Divider()
                    Text(statusMessage)
                        .font(.system(size: 12))
                        .foregroundColor(statusMessage.hasPrefix("❌") ? .red : .secondary)
                        .padding(.horizontal, 20)
                        .padding(.vertical, 8)
                }
            }
        }
        .onAppear {
            #if !APPSTORE
            state.refreshPlanRelayState()
            #endif
            guard fetchedModels.isEmpty,
                  let key = KeychainStore.shared.get("anthropic-api-key"), !key.isEmpty else { return }
            Task {
                let models = await ClaudeService.fetchModels(apiKey: key)
                guard !models.isEmpty else { return }
                await MainActor.run {
                    fetchedModels = models
                    let m = state.claudeModel
                    if models.contains(where: { $0.id == m }) {
                        modelChoice = m
                        customModel = ""
                    } else if modelChoice != Self.customModelTag {
                        modelChoice = Self.customModelTag
                        customModel = m
                    }
                }
            }
        }
    }

    // MARK: - Section routing

    private var sectionTitle: String {
        switch selectedSection {
        case "general":      return L10n.t("settings.general")
        case "activepills":  return L10n.t("settings.activePills")
        case "agents":       return L10n.t("settings.agents")
        case "chat":         return L10n.t("settings.chat")
        case "integrations": return L10n.t("settings.integrations")
        default:             return L10n.t("settings.general")
        }
    }

    @ViewBuilder private var sectionContent: some View {
        switch selectedSection {
        case "activepills":  activePillsSection
        case "agents":       agentsSection
        case "chat":         chatSection
        case "integrations": integrationsSection
        default:             generalSection
        }
    }

    // MARK: - General section

    @ViewBuilder private var generalSection: some View {
        GroupBox(L10n.t("settings.language")) {
            Picker(L10n.t("settings.language"), selection: $state.uiLanguage) {
                Text(L10n.t("settings.languageSystem")).tag("system")
                Text("Français").tag("fr")
                Text("English").tag("en")
            }
            .labelsHidden()
            .padding(6)
        }

        GroupBox(L10n.t("settings.sound")) {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(L10n.t("settings.enableSounds"), isOn: $state.soundEnabled)
                HStack(spacing: 8) {
                    Text(L10n.t("settings.volume"))
                        .frame(width: 56, alignment: .leading)
                    Slider(value: $state.soundVolume, in: 0...0.2)
                        .disabled(!state.soundEnabled)
                    Text("\(Int(state.soundVolume / 0.2 * 100)) %")
                        .frame(width: 36, alignment: .trailing)
                        .monospacedDigit()
                }
            }
            .padding(6)
        }

        GroupBox(L10n.t("settings.behavior")) {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    Text(L10n.t("settings.closeAfter"))
                    TextField("60", value: $state.autoCloseInterval, format: .number)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 64)
                    Text(L10n.t("settings.inactive"))
                }
                HStack(spacing: 8) {
                    Text(L10n.t("settings.hideAfter"))
                    TextField("3", value: absenceMinutes, format: .number)
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 48)
                    Text(L10n.t("settings.noMovement"))
                }
            }
            .padding(6)
        }

        GroupBox(L10n.t("settings.hotkeyTitle")) {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(L10n.t("settings.hotkeyToggle"), isOn: $state.hotkeyEnabled)
                if state.hotkeyEnabled {
                    HStack(spacing: 8) {
                        Text(L10n.t("settings.shortcut"))
                            .frame(width: 70, alignment: .leading)
                        ShortcutRecorderButton(flags: $hotkeyFlags, code: $hotkeyCode)
                            .onChange(of: hotkeyFlags) { _, v in state.hotkeyFlags = v }
                            .onChange(of: hotkeyCode)  { _, v in state.hotkeyCode  = v }
                        Text(L10n.t("settings.hotkeyHint"))
                            .font(.system(size: 11))
                            .foregroundColor(.secondary)
                    }
                }
            }
            .padding(6)
        }

        GroupBox(L10n.t("settings.startup")) {
            Toggle(L10n.t("settings.launchMac"), isOn: $launchAtStartup)
                .onChange(of: launchAtStartup) { _, on in toggleStartup(on) }
                .padding(6)
        }
    }

    // MARK: - Active pills section

    @ViewBuilder private var activePillsSection: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text(L10n.t("pills.choose"))
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)

                Text(L10n.t("pills.slots", ["used": "\(state.activeIntegrations.count)"]))
                    .font(.system(size: 11))
                    .foregroundColor(state.activeIntegrations.count >= 4 ? .orange : .secondary)

                Picker("Main", selection: $state.mainPillId) {
                    ForEach(PillCatalog.available.filter { $0.category == .workspace && !$0.comingSoon }, id: \.id) { def in
                        Text(def.name).tag(def.id)
                    }
                }
                .onChange(of: state.mainPillId) { _, newId in
                    state.activeIntegrations.remove(newId)
                    state.loadIntegrationTasks()
                    state.setFocus(newId)
                }

                ForEach(PillCategory.allCases, id: \.self) { cat in
                    let catPills = PillCatalog.available.filter { $0.category == cat }
                    if !catPills.isEmpty {
                        Divider()
                        Text(L10n.pillCategory(cat.rawValue))
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundColor(.secondary)
                        ForEach(catPills, id: \.id) { def in
                            pillRow(def)
                        }
                    }
                }
            }
            .padding(6)
        }
    }

    // MARK: - Agents section

    @ViewBuilder private var agentsSection: some View {
        GroupBox(L10n.t("hooks.claude")) {
            VStack(alignment: .leading, spacing: 10) {
                if hookNeedsUpdate {
                    HStack(spacing: 6) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundColor(.orange)
                        Text(L10n.t("hooks.outdated"))
                            .font(.system(size: 11))
                            .foregroundColor(.orange)
                    }
                    #if APPSTORE
                    Button(L10n.t("hooks.update")) { installHooksAppStore() }
                    #else
                    Button(L10n.t("hooks.update")) { installHooks() }
                    #endif
                }
                #if APPSTORE
                Text("~/.claude/coucou/nb-hook")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(.secondary)
                HStack(spacing: 10) {
                    Button(L10n.t("hooks.install")) { installHooksAppStore() }
                        .buttonStyle(.borderedProminent)
                    Button(L10n.t("hooks.uninstall")) { uninstallHooksAppStore() }
                        .buttonStyle(.bordered)
                }
                #else
                Text("nb-hook : \(HookServer.hookScriptPath)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(.secondary)
                HStack(spacing: 10) {
                    Button(L10n.t("hooks.install")) { installHooks() }
                        .buttonStyle(.borderedProminent)
                    Button(L10n.t("hooks.uninstall")) { uninstallHooks() }
                        .buttonStyle(.bordered)
                }
                #endif

                #if !APPSTORE
                if showDiff {
                    ScrollView {
                        Text(pendingHookJSON)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 140)
                    .background(Color(NSColor.textBackgroundColor))
                    .cornerRadius(6)

                    HStack {
                        Button(L10n.t("hooks.confirm")) { confirmInstall() }
                            .buttonStyle(.borderedProminent)
                        Button(L10n.t("common.cancel")) { showDiff = false; pendingHookJSON = "" }
                            .buttonStyle(.bordered)
                    }
                }
                #endif
            }
            .padding(6)
        }

        #if !APPSTORE
        GroupBox(L10n.t("hooks.gemini")) {
            VStack(alignment: .leading, spacing: 10) {
                Text(geminiHooksInstalled
                     ? L10n.t("hooks.geminiReady")
                     : "~/.gemini/settings.json")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(.secondary)
                HStack(spacing: 10) {
                    Button(L10n.t("hooks.install")) { triggerGeminiPreview(install: true) }
                        .buttonStyle(.borderedProminent)
                    Button(L10n.t("hooks.uninstall")) { triggerGeminiPreview(install: false) }
                        .buttonStyle(.bordered)
                }
                if showGeminiDiff {
                    ScrollView {
                        Text(pendingGeminiJSON)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 140)
                    .background(Color(NSColor.textBackgroundColor))
                    .cornerRadius(6)
                    HStack {
                        Button(L10n.t("hooks.confirm")) { confirmGeminiOp() }
                            .buttonStyle(.borderedProminent)
                        Button(L10n.t("common.cancel")) { showGeminiDiff = false; pendingGeminiJSON = "" }
                            .buttonStyle(.bordered)
                    }
                }
            }
            .padding(6)
        }

        GroupBox(L10n.t("hooks.antigravity")) {
            VStack(alignment: .leading, spacing: 10) {
                Text(agyHooksInstalled
                     ? L10n.t("hooks.agyReady")
                     : "~/.gemini/config/hooks.json")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(.secondary)
                HStack(spacing: 10) {
                    Button(L10n.t("hooks.install")) { triggerAgyPreview(install: true) }
                        .buttonStyle(.borderedProminent)
                    Button(L10n.t("hooks.uninstall")) { triggerAgyPreview(install: false) }
                        .buttonStyle(.bordered)
                }
                if showAgyDiff {
                    ScrollView {
                        Text(pendingAgyJSON)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 140)
                    .background(Color(NSColor.textBackgroundColor))
                    .cornerRadius(6)
                    HStack {
                        Button(L10n.t("hooks.confirm")) { confirmAgyOp() }
                            .buttonStyle(.borderedProminent)
                        Button(L10n.t("common.cancel")) { showAgyDiff = false; pendingAgyJSON = "" }
                            .buttonStyle(.bordered)
                    }
                }
            }
            .padding(6)
        }

        GroupBox(L10n.t("hooks.codex")) {
            VStack(alignment: .leading, spacing: 10) {
                Text(codexHooksInstalled
                     ? L10n.t("hooks.codexReady")
                     : "~/.codex/hooks.json")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(.secondary)
                HStack(spacing: 10) {
                    Button(L10n.t("hooks.install")) { triggerCodexPreview(install: true) }
                        .buttonStyle(.borderedProminent)
                    Button(L10n.t("hooks.uninstall")) { triggerCodexPreview(install: false) }
                        .buttonStyle(.bordered)
                }
                if showCodexDiff {
                    ScrollView {
                        Text(pendingCodexJSON)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 140)
                    .background(Color(NSColor.textBackgroundColor))
                    .cornerRadius(6)
                    HStack {
                        Button(L10n.t("hooks.confirm")) { confirmCodexOp() }
                            .buttonStyle(.borderedProminent)
                        Button(L10n.t("common.cancel")) { showCodexDiff = false; pendingCodexJSON = "" }
                            .buttonStyle(.bordered)
                    }
                }
            }
            .padding(6)
        }

        GroupBox(L10n.t("hooks.copilot")) {
            VStack(alignment: .leading, spacing: 10) {
                Text(copilotHooksInstalled
                     ? L10n.t("hooks.copilotReady")
                     : "~/.copilot/hooks/coucou.json")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundColor(.secondary)
                HStack(spacing: 10) {
                    Button(L10n.t("hooks.install")) { triggerCopilotPreview(install: true) }
                        .buttonStyle(.borderedProminent)
                    Button(L10n.t("hooks.uninstall")) { triggerCopilotPreview(install: false) }
                        .buttonStyle(.bordered)
                }
                if showCopilotDiff {
                    ScrollView {
                        Text(pendingCopilotJSON)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 140)
                    .background(Color(NSColor.textBackgroundColor))
                    .cornerRadius(6)
                    HStack {
                        Button(L10n.t("hooks.confirm")) { confirmCopilotOp() }
                            .buttonStyle(.borderedProminent)
                        Button(L10n.t("common.cancel")) { showCopilotDiff = false; pendingCopilotJSON = "" }
                            .buttonStyle(.bordered)
                    }
                }
            }
            .padding(6)
        }

        GroupBox(L10n.t("plan.title")) {
            VStack(alignment: .leading, spacing: 10) {
                Text(L10n.t("plan.blurb"))
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Toggle(L10n.t("plan.show"), isOn: Binding(
                    get: { state.showPlanInNotch || planTogglePending },
                    set: { on in
                        if on {
                            if state.planRelayInstalled {
                                state.showPlanInNotch = true
                            } else {
                                planTogglePending = true
                                installStatusLine()
                            }
                        } else {
                            state.showPlanInNotch = false
                            planTogglePending = false
                        }
                    }
                ))
                HStack(spacing: 10) {
                    if state.planRelayInstalled {
                        Text(L10n.t("plan.relayOn"))
                            .font(.system(size: 11))
                            .foregroundColor(.secondary)
                        Button(L10n.t("plan.uninstall")) { uninstallStatusLine() }
                            .buttonStyle(.bordered)
                    } else {
                        Text(L10n.t("plan.relayOff"))
                            .font(.system(size: 11))
                            .foregroundColor(.secondary)
                        Button(L10n.t("plan.install")) { installStatusLine() }
                            .buttonStyle(.borderedProminent)
                    }
                }
                if showStatusLineDiff {
                    ScrollView {
                        Text(pendingStatusLineJSON)
                            .font(.system(size: 10, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(height: 100)
                    .background(Color(NSColor.textBackgroundColor))
                    .cornerRadius(6)
                    HStack {
                        Button(L10n.t("hooks.confirm")) { confirmStatusLine() }
                            .buttonStyle(.borderedProminent)
                        Button(L10n.t("common.cancel")) {
                            showStatusLineDiff = false
                            pendingStatusLineJSON = ""
                            planTogglePending = false
                        }
                        .buttonStyle(.bordered)
                    }
                }
            }
            .padding(6)
        }
        #endif
    }

    // MARK: - Chat section

    @ViewBuilder private var chatSection: some View {
        GroupBox(L10n.t("chat.anthropic")) {
            VStack(alignment: .leading, spacing: 8) {
                SecureField(L10n.t("chat.keyPlaceholder"), text: $apiKey)
                    .textFieldStyle(.roundedBorder)
                Button(L10n.t("common.save")) {
                    KeychainStore.shared.set("anthropic-api-key", value: apiKey)
                    statusMessage = L10n.t("status.keySaved")
                }
                .buttonStyle(.borderedProminent)

                Divider().padding(.vertical, 2)

                Picker("Model", selection: $modelChoice) {
                    ForEach(displayModels, id: \.id) { preset in
                        Text(preset.label).tag(preset.id)
                    }
                    Text(L10n.t("chat.custom")).tag(Self.customModelTag)
                }
                .onChange(of: modelChoice) { _, choice in
                    if choice != Self.customModelTag {
                        state.claudeModel = choice
                    } else {
                        applyCustomModel(customModel)
                    }
                }

                if modelChoice == Self.customModelTag {
                    TextField(L10n.t("chat.modelId"), text: $customModel)
                        .textFieldStyle(.roundedBorder)
                        .onChange(of: customModel) { _, value in applyCustomModel(value) }
                }

                Text(L10n.t("chat.usedBy"))
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
            }
            .padding(6)
        }

        GroupBox(L10n.t("chat.other")) {
            VStack(alignment: .leading, spacing: 12) {
                Text(L10n.t("chat.otherHint"))
                    .font(.system(size: 12))
                    .foregroundColor(.secondary)

                HStack(spacing: 8) {
                    Circle().fill(Color(hex: "#4285F4")).frame(width: 8, height: 8)
                    Text("Google AI").font(.system(size: 12, weight: .semibold))
                }
                SecureField(L10n.t("chat.googleKey"), text: $googleKey)
                    .textFieldStyle(.roundedBorder)
                Button(L10n.t("common.save")) {
                    KeychainStore.shared.set("google-api-key", value: googleKey)
                    statusMessage = L10n.t("status.googleSaved")
                }
                .buttonStyle(.borderedProminent)

                Divider()

                HStack(spacing: 8) {
                    Circle().fill(Color(hex: "#10A37F")).frame(width: 8, height: 8)
                    Text("OpenAI").font(.system(size: 12, weight: .semibold))
                }
                SecureField(L10n.t("chat.openaiKey"), text: $openAIKey)
                    .textFieldStyle(.roundedBorder)
                Button(L10n.t("common.save")) {
                    KeychainStore.shared.set("openai-api-key", value: openAIKey)
                    statusMessage = L10n.t("status.openaiSaved")
                }
                .buttonStyle(.borderedProminent)
            }
            .padding(.vertical, 4)
        }

        GroupBox(L10n.t("chat.local")) {
            VStack(alignment: .leading, spacing: 12) {
                Text(L10n.t("chat.localHint"))
                    .font(.system(size: 12))
                    .foregroundColor(.secondary)

                // ── Ollama ──────────────────────────────────────────────────────
                HStack(spacing: 8) {
                    Circle().fill(Color(hex: "#FACC15")).frame(width: 8, height: 8)
                    Text("Ollama").font(.system(size: 12, weight: .semibold))
                    if !state.ollamaServerURL.isEmpty {
                        Text(L10n.t("chat.connected"))
                            .font(.system(size: 10))
                            .foregroundColor(Color(hex: "#22C55E"))
                    }
                }
                if state.ollamaServerURL.isEmpty {
                    TextField("http://127.0.0.1:11434", text: $ollamaURL)
                        .textFieldStyle(.roundedBorder)
                    Button(connectingOllama ? L10n.t("chat.connecting") : L10n.t("chat.connect")) {
                        Task { await connectLocal(provider: .ollama) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(connectingOllama)
                } else {
                    Text(state.ollamaServerURL)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundColor(.secondary)
                    Button(L10n.t("chat.disconnect")) {
                        state.ollamaServerURL = ""
                        ollamaURL = ""
                        state.fetchedProviderModels[.ollama] = nil
                        state.providerModelFetchError[.ollama] = nil
                        if state.chatProvider == .ollama { state.chatProvider = .anthropic }
                        statusMessage = L10n.t("status.ollamaOff")
                    }
                    .buttonStyle(.bordered)
                }

                Divider()

                // ── LM Studio ───────────────────────────────────────────────────
                HStack(spacing: 8) {
                    Circle().fill(Color(hex: "#A3E635")).frame(width: 8, height: 8)
                    Text("LM Studio").font(.system(size: 12, weight: .semibold))
                    if !state.lmstudioServerURL.isEmpty {
                        Text(L10n.t("chat.connected"))
                            .font(.system(size: 10))
                            .foregroundColor(Color(hex: "#22C55E"))
                    }
                }
                if state.lmstudioServerURL.isEmpty {
                    TextField("http://127.0.0.1:1234", text: $lmstudioURL)
                        .textFieldStyle(.roundedBorder)
                    Button(connectingLMStudio ? L10n.t("chat.connecting") : L10n.t("chat.connect")) {
                        Task { await connectLocal(provider: .lmstudio) }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(connectingLMStudio)
                } else {
                    Text(state.lmstudioServerURL)
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundColor(.secondary)
                    Button(L10n.t("chat.disconnect")) {
                        state.lmstudioServerURL = ""
                        lmstudioURL = ""
                        state.fetchedProviderModels[.lmstudio] = nil
                        state.providerModelFetchError[.lmstudio] = nil
                        if state.chatProvider == .lmstudio { state.chatProvider = .anthropic }
                        statusMessage = L10n.t("status.lmstudioOff")
                    }
                    .buttonStyle(.bordered)
                }
            }
            .padding(.vertical, 4)
        }
    }

    // MARK: - Integrations section

    @ViewBuilder private var integrationsSection: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 14) {

                // Resend
                VStack(alignment: .leading, spacing: 5) {
                    HStack(spacing: 6) {
                        Circle().fill(Color(hex: "#22C55E")).frame(width: 8, height: 8)
                        Text("Resend").font(.system(size: 12, weight: .semibold))
                    }
                    SecureField(L10n.t("int.resendKey"), text: $resendKey)
                        .textFieldStyle(.roundedBorder)
                    TextField(L10n.t("int.resendFrom"), text: $resendFrom)
                        .textFieldStyle(.roundedBorder)
                }

                // n8n
                VStack(alignment: .leading, spacing: 5) {
                    HStack(spacing: 6) {
                        Circle().fill(Color(hex: "#F29B38")).frame(width: 8, height: 8)
                        Text("n8n").font(.system(size: 12, weight: .semibold))
                    }
                    TextField(L10n.t("int.n8nUrl"), text: $n8nUrl)
                        .textFieldStyle(.roundedBorder)
                    SecureField(L10n.t("chat.key"), text: $n8nKey)
                        .textFieldStyle(.roundedBorder)
                    IntegrationFilterRow(
                        label: L10n.t("filter.workflows"),
                        items: n8nWorkflows,
                        filter: $state.n8nWorkflowFilter,
                        loading: loadingN8n,
                        onLoad: loadN8nWorkflows
                    )
                }

                // Vercel
                VStack(alignment: .leading, spacing: 5) {
                    HStack(spacing: 6) {
                        Circle().fill(Color(hex: "#7C5CFF")).frame(width: 8, height: 8)
                        Text("Vercel").font(.system(size: 12, weight: .semibold))
                    }
                    SecureField(L10n.t("int.token"), text: $vercelToken)
                        .textFieldStyle(.roundedBorder)
                    IntegrationFilterRow(
                        label: L10n.t("filter.projects"),
                        items: vercelProjects,
                        filter: $state.vercelProjectFilter,
                        loading: loadingVercel,
                        onLoad: loadVercelProjects
                    )
                }

                // GitHub
                VStack(alignment: .leading, spacing: 5) {
                    HStack(spacing: 6) {
                        Circle().fill(Color(hex: "#F4505E")).frame(width: 8, height: 8)
                        Text("GitHub").font(.system(size: 12, weight: .semibold))
                    }
                    SecureField(L10n.t("int.personalToken"), text: $githubToken)
                        .textFieldStyle(.roundedBorder)
                    Text(L10n.t("int.githubHint"))
                        .font(.system(size: 10))
                        .foregroundColor(Color(hex: "#8E939C"))
                }

                // Stripe
                VStack(alignment: .leading, spacing: 5) {
                    HStack(spacing: 6) {
                        Circle().fill(Color(hex: "#0570DE")).frame(width: 8, height: 8)
                        Text("Stripe").font(.system(size: 12, weight: .semibold))
                    }
                    SecureField(L10n.t("int.stripeKey"), text: $stripeKey)
                        .textFieldStyle(.roundedBorder)
                }

                // Cal.com
                VStack(alignment: .leading, spacing: 5) {
                    HStack(spacing: 6) {
                        Circle().fill(Color(hex: "#C9956A")).frame(width: 8, height: 8)
                        Text("Cal.com").font(.system(size: 12, weight: .semibold))
                    }
                    SecureField(L10n.t("int.calcomKey"), text: $calcomKey)
                        .textFieldStyle(.roundedBorder)
                }

                // Notion
                VStack(alignment: .leading, spacing: 5) {
                    HStack(spacing: 6) {
                        Circle().fill(Color(hex: "#E8E8E8")).frame(width: 8, height: 8)
                        Text("Notion").font(.system(size: 12, weight: .semibold))
                    }
                    SecureField(L10n.t("int.notionKey"), text: $notionKey)
                        .textFieldStyle(.roundedBorder)
                }

                Button(L10n.t("int.save")) { saveIntegrations() }
                    .buttonStyle(.borderedProminent)
            }
            .padding(6)
        }
    }

    // MARK: - Actions

    private func applyCustomModel(_ value: String) {
        let id = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if !id.isEmpty { state.claudeModel = id }
    }

    private func toggleStartup(_ on: Bool) {
        do {
            if on { try SMAppService.mainApp.register() }
            else  { try SMAppService.mainApp.unregister() }
        } catch {
            statusMessage = L10n.t("status.startup", ["message": error.localizedDescription])
            launchAtStartup = !on
        }
    }

    // MARK: - App Store: hooks via NSOpenPanel + security-scoped bookmark

    #if APPSTORE
    private func pickClaudeFolder(prompt: String) -> URL? {
        let panel = NSOpenPanel()
        panel.message = "Select your .claude folder (press ⇧⌘. to show hidden files)"
        panel.prompt = prompt
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.showsHiddenFiles = true
        let realHomePath = getpwuid(getuid()).flatMap { String(cString: $0.pointee.pw_dir, encoding: .utf8) }
            ?? "/Users/\(NSUserName())"
        panel.directoryURL = URL(fileURLWithPath: realHomePath)
        guard panel.runModal() == .OK, let url = panel.url else { return nil }
        guard url.lastPathComponent == ".claude" else {
            statusMessage = L10n.t("status.selectFolder")
            return nil
        }
        return url
    }

    private func installHooksAppStore() {
        guard let claudeURL = pickClaudeFolder(prompt: L10n.t("alert.select")) else { return }
        let alert = NSAlert()
        alert.messageText = L10n.t("alert.installTitle")
        alert.informativeText = L10n.t("alert.installBody")
        alert.addButton(withTitle: L10n.t("alert.install"))
        alert.addButton(withTitle: L10n.t("alert.cancel"))
        alert.alertStyle = .informational
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        do {
            try HookServer.shared.installAndWriteClaudeHooksAppStore(claudeURL: claudeURL)
            hookNeedsUpdate = false
            statusMessage = L10n.t("status.hooksVSCode")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func uninstallHooksAppStore() {
        guard let claudeURL = pickClaudeFolder(prompt: L10n.t("alert.select")) else { return }
        do {
            try HookServer.shared.uninstallClaudeHooksAppStore(claudeURL: claudeURL)
            statusMessage = L10n.t("status.hooksRemoved")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }
    #endif

    private func connectLocal(provider: ChatProvider) async {
        let rawURL = provider == .ollama ? ollamaURL : lmstudioURL
        let candidate = rawURL.isEmpty
            ? (provider == .ollama ? "http://127.0.0.1:11434" : "http://127.0.0.1:1234")
            : rawURL
        let normalised = LocalChat.normaliseURL(candidate)
        guard normalised.hasPrefix("http://") || normalised.hasPrefix("https://") else {
            statusMessage = L10n.t("status.httpOnly")
            return
        }
        if provider == .ollama { connectingOllama = true } else { connectingLMStudio = true }
        statusMessage = ""
        let result = await LocalChat.fetchModelsResult(baseURL: normalised)
        if provider == .ollama { connectingOllama = false } else { connectingLMStudio = false }
        let name = provider == .ollama ? "Ollama" : "LM Studio"
        switch result {
        case .success(let models) where models.isEmpty:
            statusMessage = L10n.t("status.noModels", ["name": name])
        case .success(let models):
            if provider == .ollama {
                state.ollamaServerURL = normalised
                ollamaURL = normalised
                state.fetchedProviderModels[.ollama] = nil
                state.providerModelFetchError[.ollama] = nil
            } else {
                state.lmstudioServerURL = normalised
                lmstudioURL = normalised
                state.fetchedProviderModels[.lmstudio] = nil
                state.providerModelFetchError[.lmstudio] = nil
            }
            statusMessage = L10n.t(models.count == 1 ? "status.connectedOne" : "status.connectedMany", ["count": "\(models.count)"])
        case .failure:
            statusMessage = L10n.t("status.unreachable", ["name": name, "url": normalised])
        }
    }

    private func installHooks() {
        do {
            pendingHookJSON = try HookServer.shared.previewClaudeHooks()
            showDiff = true
            statusMessage = L10n.t("status.review")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func confirmInstall() {
        do {
            try HookServer.shared.writeClaudeHooks()
            showDiff = false
            statusMessage = L10n.t("status.hooksWritten")
            pendingHookJSON = ""
            hookNeedsUpdate = false
        } catch {
            statusMessage = L10n.t("status.writeError", ["message": error.localizedDescription])
        }
    }

    private func uninstallHooks() {
        do {
            try HookServer.shared.uninstallClaudeHooks()
            statusMessage = L10n.t("status.hooksRemoved")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    #if !APPSTORE
    private func triggerGeminiPreview(install: Bool) {
        do {
            geminiPendingInstall = install
            pendingGeminiJSON = try HookServer.shared.previewGeminiHooks(install: install)
            showGeminiDiff = true
            statusMessage = L10n.t("status.review")
        } catch let e as NSError where e.domain == "CoucouNoop" {
            statusMessage = e.localizedDescription
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func confirmGeminiOp() {
        do {
            try HookServer.shared.writeGeminiHooks()
            showGeminiDiff = false
            pendingGeminiJSON = ""
            geminiHooksInstalled = geminiPendingInstall
            statusMessage = geminiPendingInstall
                ? L10n.t("status.geminiOn")
                : L10n.t("status.geminiOff")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func triggerAgyPreview(install: Bool) {
        do {
            agyPendingInstall = install
            pendingAgyJSON = try HookServer.shared.previewAgyHooks(install: install)
            showAgyDiff = true
            statusMessage = L10n.t("status.review")
        } catch let e as NSError where e.domain == "CoucouNoop" {
            statusMessage = e.localizedDescription
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func confirmAgyOp() {
        do {
            try HookServer.shared.writeAgyHooks()
            showAgyDiff = false
            pendingAgyJSON = ""
            agyHooksInstalled = agyPendingInstall
            statusMessage = agyPendingInstall
                ? L10n.t("status.agyOn")
                : L10n.t("status.agyOff")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func triggerCodexPreview(install: Bool) {
        do {
            codexPendingInstall = install
            pendingCodexJSON = try HookServer.shared.previewCodexHooks(install: install)
            showCodexDiff = true
            statusMessage = L10n.t("status.review")
        } catch let e as NSError where e.domain == "CoucouNoop" {
            statusMessage = e.localizedDescription
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func confirmCodexOp() {
        do {
            try HookServer.shared.writeCodexHooks()
            showCodexDiff = false
            pendingCodexJSON = ""
            codexHooksInstalled = codexPendingInstall
            statusMessage = codexPendingInstall
                ? L10n.t("status.codexOn")
                : L10n.t("status.codexOff")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func triggerCopilotPreview(install: Bool) {
        do {
            copilotPendingInstall = install
            pendingCopilotJSON = try HookServer.shared.previewCopilotHooks(install: install)
            showCopilotDiff = true
            statusMessage = L10n.t("status.review")
        } catch let e as NSError where e.domain == "CoucouNoop" {
            statusMessage = e.localizedDescription
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func confirmCopilotOp() {
        do {
            try HookServer.shared.writeCopilotHooks()
            showCopilotDiff = false
            pendingCopilotJSON = ""
            copilotHooksInstalled = copilotPendingInstall
            statusMessage = copilotPendingInstall
                ? L10n.t("status.copilotOn")
                : L10n.t("status.copilotOff")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func installStatusLine() {
        do {
            pendingStatusLineJSON = try HookServer.shared.previewStatusLine(install: true)
            showStatusLineDiff = true
            statusLinePendingInstall = true
            statusMessage = L10n.t("status.review")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func uninstallStatusLine() {
        do {
            pendingStatusLineJSON = try HookServer.shared.previewStatusLine(install: false)
            showStatusLineDiff = true
            statusLinePendingInstall = false
            statusMessage = L10n.t("status.review")
        } catch {
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }

    private func confirmStatusLine() {
        do {
            try HookServer.shared.writeStatusLine()
            showStatusLineDiff = false
            pendingStatusLineJSON = ""
            state.refreshPlanRelayState()
            if planTogglePending {
                state.showPlanInNotch = true
                planTogglePending = false
            }
            if !statusLinePendingInstall {
                state.showPlanInNotch = false
            }
            statusMessage = statusLinePendingInstall
                ? L10n.t("status.lineOn")
                : L10n.t("status.lineOff")
        } catch {
            planTogglePending = false
            statusMessage = L10n.t("status.fail", ["message": error.localizedDescription])
        }
    }
    #endif

    private func saveIntegrations() {
        saveKey("resend-api-key",  value: resendKey)
        saveKey("resend-from",     value: resendFrom)
        saveKey("n8n-url",         value: n8nUrl)
        saveKey("n8n-api-key",     value: n8nKey)
        saveKey("vercel-token",    value: vercelToken)

        // Detect GitHub token changes before writing
        let prevGithubToken = KeychainStore.shared.get("github-token")
        saveKey("github-token", value: githubToken)
        let nextGithubToken = KeychainStore.shared.get("github-token")
        if nextGithubToken != prevGithubToken {
            AppState.shared.githubPulse = nil
            AppState.shared.githubActivity = nil
            if nextGithubToken == nil { AppState.shared.githubStats = nil }
            if nextGithubToken != nil {
                GithubPoller.shared.triggerPulseNow()
                GithubPoller.shared.refreshActivityIfStale()
            }
        }

        saveKey("stripe-api-key",  value: stripeKey)
        saveKey("calcom-api-key",  value: calcomKey)
        saveKey("notion-api-key",  value: notionKey)
        statusMessage = L10n.t("status.integrationsSaved")
    }

    private func saveKey(_ key: String, value: String) {
        if value.isEmpty {
            KeychainStore.shared.remove(key)
        } else {
            KeychainStore.shared.set(key, value: value)
        }
    }

    // MARK: - Vercel project list

    private func loadVercelProjects() {
        guard let token = KeychainStore.shared.get("vercel-token") else {
            statusMessage = L10n.t("status.vercelFirst")
            return
        }
        loadingVercel = true
        guard let url = URL(string: "https://api.vercel.com/v9/projects?limit=100") else { return }
        var req = URLRequest(url: url, timeoutInterval: 10)
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        URLSession.shared.dataTask(with: req) { data, response, _ in
            let names: [String]
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let projects = json["projects"] as? [[String: Any]] {
                names = projects.compactMap { $0["name"] as? String }.sorted()
            } else {
                names = []
            }
            DispatchQueue.main.async {
                self.vercelProjects = names
                self.loadingVercel = false
                if names.isEmpty { self.statusMessage = L10n.t("status.vercelNone") }
            }
        }.resume()
    }

    // MARK: - n8n workflow list

    private func loadN8nWorkflows() {
        guard let apiKey  = KeychainStore.shared.get("n8n-api-key"),
              let rawBase = KeychainStore.shared.get("n8n-url") else {
            statusMessage = L10n.t("status.n8nFirst")
            return
        }
        loadingN8n = true
        let base = rawBase.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let urls = ["\(base)/api/v1/workflows?limit=100", "\(base)/rest/workflows?limit=100"]
        fetchN8nWorkflows(urls: urls, apiKey: apiKey, idx: 0)
    }

    private func fetchN8nWorkflows(urls: [String], apiKey: String, idx: Int) {
        guard idx < urls.count, let url = URL(string: urls[idx]) else {
            DispatchQueue.main.async { self.loadingN8n = false; self.statusMessage = L10n.t("status.n8nNone") }
            return
        }
        var req = URLRequest(url: url, timeoutInterval: 10)
        req.setValue(apiKey, forHTTPHeaderField: "X-N8N-API-KEY")
        URLSession.shared.dataTask(with: req) { data, response, _ in
            let code = (response as? HTTPURLResponse)?.statusCode ?? 0
            guard let data, code == 200 else {
                self.fetchN8nWorkflows(urls: urls, apiKey: apiKey, idx: idx + 1)
                return
            }
            let items: [[String: Any]]
            if let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
               let arr = obj["data"] as? [[String: Any]] { items = arr }
            else if let arr = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]] { items = arr }
            else { items = [] }
            let names = items.compactMap { $0["name"] as? String }.sorted()
            DispatchQueue.main.async {
                self.n8nWorkflows = names
                self.loadingN8n = false
                if names.isEmpty { self.statusMessage = L10n.t("status.n8nNone") }
            }
        }.resume()
    }

    @ViewBuilder
    private func pillRow(_ def: PillDefinition) -> some View {
        let isMain = def.id == state.mainPillId
        let isOn   = state.activeIntegrations.contains(def.id)
        let atMax  = state.activeIntegrations.count >= 4 && !isOn && !isMain
        let hint: String? = {
            if isMain { return nil }
            if def.comingSoon { return L10n.t("int.comingSoon") }
            #if !APPSTORE
            if def.id == "agent_gemini"        && !HookServer.geminiHooksInstalled()  { return L10n.t("int.hooksMissing") }
            if def.id == "agent_antigravity"   && !HookServer.agyHooksInstalled()    { return L10n.t("int.hooksMissing") }
            if def.id == "agent_codex"         && !HookServer.codexHooksInstalled()  { return L10n.t("int.hooksMissing") }
            if def.id == "agent_copilot"       && !HookServer.copilotHooksInstalled() { return L10n.t("int.hooksMissing") }
            #endif
            if def.category == .ai {
                if let provider = ChatProvider(pillID: def.id), provider.isLocal {
                    let url = provider == .ollama ? state.ollamaServerURL : state.lmstudioServerURL
                    if url.isEmpty { return L10n.t("int.notConnected") }
                } else {
                    let keyId = def.id == "ai_anthropic" ? "anthropic-api-key"
                               : def.id == "ai_google"    ? "google-api-key" : "openai-api-key"
                    if KeychainStore.shared.get(keyId) == nil { return L10n.t("int.keyMissing") }
                }
            }
            return nil
        }()
        HStack(spacing: 8) {
            Circle()
                .fill(Color(hex: def.color))
                .frame(width: 10, height: 10)
            Text(def.name)
                .font(.system(size: 12))
                .foregroundColor(atMax ? .secondary : .primary)
            Spacer()
            if isMain {
                Text(L10n.t("pills.main"))
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
            } else {
                if let h = hint {
                    Text(h)
                        .font(.system(size: 11))
                        .foregroundColor(.secondary)
                }
                Toggle("", isOn: Binding(
                    get: { isOn },
                    set: { _ in state.toggleIntegration(def.id) }
                ))
                .labelsHidden()
                .disabled(atMax)
            }
        }
    }
}

// MARK: - Sidebar background (NSVisualEffectView .sidebar)

struct SidebarBackground: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = .sidebar
        v.blendingMode = .behindWindow
        v.state = .active
        return v
    }
    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {}
}

// MARK: - Sidebar row (System Settings style icon)

struct SettingsSidebarRow: View {
    let title: String
    let icon: String
    let color: String

    var body: some View {
        Label {
            Text(title)
        } icon: {
            Image(systemName: icon)
                .font(.system(size: 11, weight: .semibold))
                .foregroundColor(.white)
                .frame(width: 20, height: 20)
                .background(RoundedRectangle(cornerRadius: 5).fill(Color(hex: color)))
        }
    }
}

// MARK: - Integration filter row (reusable for Vercel / n8n)

struct IntegrationFilterRow: View {
    let label: String
    let items: [String]
    @Binding var filter: Set<String>
    let loading: Bool
    let onLoad: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Text(label)
                    .font(.system(size: 11))
                    .foregroundColor(.secondary)
                Spacer()
                if loading {
                    ProgressView().scaleEffect(0.6)
                } else {
                    Button(items.isEmpty ? L10n.t("github.load") : L10n.t("int.refresh")) { onLoad() }
                        .buttonStyle(.bordered)
                        .controlSize(.mini)
                }
                if !filter.isEmpty {
                    Button(L10n.t("github.clear")) { filter = [] }
                        .buttonStyle(.bordered)
                        .controlSize(.mini)
                        .foregroundColor(.secondary)
                }
            }
            if !items.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(items, id: \.self) { item in
                        Toggle(item, isOn: Binding(
                            get: { filter.isEmpty || filter.contains(item) },
                            set: { on in
                                if on { filter.insert(item) }
                                else  {
                                    if filter.isEmpty { filter = Set(items).subtracting([item]) }
                                    else { filter.remove(item) }
                                    if filter.count == items.count { filter = [] }
                                }
                            }
                        ))
                        .font(.system(size: 11))
                        .toggleStyle(.checkbox)
                    }
                }
                .padding(.leading, 4)
                if !filter.isEmpty {
                    Text(L10n.t("github.watching", ["count": "\(filter.count)", "total": "\(items.count)"]))
                        .font(.system(size: 10))
                        .foregroundColor(.secondary)
                }
            }
        }
    }
}

// MARK: - Shortcut recorder button

struct ShortcutRecorderButton: View {
    @Binding var flags: UInt
    @Binding var code: UInt16
    @State private var isRecording = false

    var body: some View {
        Button {
            guard !isRecording else { return }
            isRecording = true
            var token: Any?
            token = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
                let mods = event.modifierFlags.intersection([.command, .control, .option, .shift])
                guard !mods.isEmpty else { return event }
                DispatchQueue.main.async {
                    self.flags = mods.rawValue
                    self.code = event.keyCode
                    self.isRecording = false
                    if let t = token { NSEvent.removeMonitor(t) }
                }
                return nil
            }
        } label: {
            Text(isRecording ? L10n.t("shortcut.press") : shortcutLabel)
                .font(.system(size: 11, design: .monospaced))
                .padding(.horizontal, 8).padding(.vertical, 3)
                .background(isRecording ? Color.accentColor.opacity(0.12) : Color(NSColor.controlBackgroundColor))
                .cornerRadius(5)
                .overlay(RoundedRectangle(cornerRadius: 5).stroke(Color.gray.opacity(0.3), lineWidth: 1))
        }
        .buttonStyle(.plain)
    }

    private var shortcutLabel: String {
        let f = NSEvent.ModifierFlags(rawValue: flags)
        var s = ""
        if f.contains(.control) { s += "⌃" }
        if f.contains(.option)  { s += "⌥" }
        if f.contains(.shift)   { s += "⇧" }
        if f.contains(.command) { s += "⌘" }
        s += keyChar(code)
        return s.isEmpty ? L10n.t("shortcut.none") : s
    }

    private func keyChar(_ c: UInt16) -> String {
        let map: [UInt16: String] = [
            0:"A", 1:"S", 2:"D", 3:"F", 4:"H", 5:"G", 6:"Z", 7:"X", 8:"C", 9:"V",
            11:"B", 12:"Q", 13:"W", 14:"E", 15:"R", 16:"Y", 17:"T", 31:"O", 32:"U",
            34:"I", 37:"L", 38:"J", 40:"K", 45:"N", 46:"M", 49:"Space", 50:"`", 27:"-"
        ]
        return map[c] ?? "·"
    }
}
