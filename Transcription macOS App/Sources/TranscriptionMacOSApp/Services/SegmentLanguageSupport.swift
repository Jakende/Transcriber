import Foundation
import NaturalLanguage

enum SegmentLanguageSupport {
    static func detect(in text: String) -> String? {
        let clean = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard clean.count >= 12 else { return nil }
        let recognizer = NLLanguageRecognizer()
        recognizer.processString(clean)
        let hypotheses = recognizer.languageHypotheses(withMaximum: 2)
        let supported = hypotheses.filter { $0.key == .german || $0.key == .english }
        guard let best = supported.max(by: { $0.value < $1.value }), best.value >= 0.55 else { return nil }
        return best.key == .german ? "de" : "en"
    }

    static func annotate(_ document: inout TranscriptDocument) {
        for index in document.segments.indices where document.segments[index].language == nil {
            document.segments[index].language = detect(in: document.segments[index].text)
        }
    }

    static func title(for code: String?) -> String? {
        switch code {
        case "de": return "DE"
        case "en": return "EN"
        default: return nil
        }
    }
}
