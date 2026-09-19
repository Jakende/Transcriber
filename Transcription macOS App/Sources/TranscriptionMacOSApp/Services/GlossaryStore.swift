import Foundation

enum GlossaryStore {
    private static let key = "transcription.confirmedGlossaryTerms"

    static func terms() -> [String] {
        UserDefaults.standard.stringArray(forKey: key) ?? []
    }

    static func add(replacements: [String: String]) {
        var values = Set(terms())
        for (source, replacement) in replacements {
            let term = replacement.trimmingCharacters(in: .whitespacesAndNewlines)
            if !term.isEmpty && term != source { values.insert(term) }
        }
        save(Array(values))
    }

    static func save(_ terms: [String]) {
        let values = Set(terms.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty })
        UserDefaults.standard.set(values.sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }, forKey: key)
    }

    static func prompt(maxCharacters: Int = 1_000) -> String? {
        let prefix = "Wichtige Namen und Fachbegriffe: "
        var result = prefix
        for term in terms() {
            let addition = (result == prefix ? "" : ", ") + term
            guard result.count + addition.count <= maxCharacters else { break }
            result += addition
        }
        return result == prefix ? nil : result
    }
}
