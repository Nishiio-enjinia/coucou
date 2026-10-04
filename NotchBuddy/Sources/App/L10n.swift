import Foundation

/// Shared catalogs in `locales/`. English is the source and the fallback.
/// Add every new key to both `locales/en.json` and `locales/fr.json`.
/// `preference` is "system", "en" or "fr". Placeholders are `{name}`.
enum L10n {
    private static let tables: [String: [String: String]] = [
        "en": load("en"),
        "fr": load("fr"),
    ]

    /// Set from the language option. "system" follows the OS.
    static var preference = "system"

    static var language: String {
        if preference == "en" || preference == "fr" { return preference }
        let code = Locale.current.language.languageCode?.identifier ?? "en"
        return tables[code] != nil ? code : "en"
    }

    static var locale: Locale { Locale(identifier: language) }

    static func t(_ key: String, _ vars: [String: String] = [:]) -> String {
        var value = tables[language]?[key] ?? tables["en"]?[key] ?? key
        for (name, replacement) in vars {
            value = value.replacingOccurrences(of: "{\(name)}", with: replacement)
        }
        return value
    }

    /// Compact relative time used on integration rows. `brief` is the Notion "now".
    static func ago(since date: Date, brief: Bool = false) -> String {
        let diff = Date().timeIntervalSince(date)
        if diff < 60 { return t(brief ? "time.nowBrief" : "time.now") }
        if diff < 3600 { return t("time.m", ["n": String(Int(diff / 60))]) }
        if diff < 86400 { return t("time.h", ["n": String(Int(diff / 3600))]) }
        return t("time.d", ["n": String(Int(diff / 86400))])
    }

    /// Pill subtitles stored in English on the catalog. Product names stay as-is.
    static func pillSubtitle(_ english: String) -> String {
        switch english {
        case "Integration": return t("pill.sub.integration")
        case "Agent":       return t("pill.sub.agent")
        case "Chat":        return t("pill.sub.chat")
        case "Claude Code": return t("pill.sub.claude")
        case "Cursor":      return t("pill.sub.cursor")
        case "Codex":       return t("pill.sub.codex")
        default:            return english
        }
    }

    static func pillCategory(_ raw: String) -> String {
        t("pills.\(raw)")
    }

    private static func load(_ name: String) -> [String: String] {
        guard let url = Bundle.main.url(forResource: name, withExtension: "json", subdirectory: "locales"),
              let data = try? Data(contentsOf: url),
              let table = try? JSONDecoder().decode([String: String].self, from: data) else {
            return [:]
        }
        return table
    }
}
