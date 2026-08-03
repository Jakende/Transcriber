import Foundation

enum GlossarySupport {
    static func merged(_ candidates: [GlossaryCandidate]) -> [GlossaryCandidate] {
        let cleaned = candidates.compactMap { candidate -> GlossaryCandidate? in
            let term = candidate.term.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !term.isEmpty else { return nil }
            return GlossaryCandidate(term: term, count: max(0, candidate.count), kind: candidate.kind)
        }
        let groups = Dictionary(grouping: cleaned, by: { normalizedKey($0.term) })
        return groups.values.map { group in
            var variantCounts: [String: Int] = [:]
            var variantOrder: [String] = []
            for candidate in group {
                if variantCounts[candidate.term] == nil { variantOrder.append(candidate.term) }
                variantCounts[candidate.term, default: 0] += candidate.count
            }
            var preferredTerm = variantOrder[0]
            for term in variantOrder.dropFirst() where variantCounts[term, default: 0] > variantCounts[preferredTerm, default: 0] {
                preferredTerm = term
            }
            let kinds = Set(group.flatMap { candidate in
                candidate.kind.split(separator: "/").map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            }.filter { !$0.isEmpty }).sorted()
            return GlossaryCandidate(
                term: preferredTerm,
                count: group.reduce(0) { $0 + $1.count },
                kind: kinds.isEmpty ? "PROPN" : kinds.joined(separator: " / ")
            )
        }.sorted {
            if $0.count != $1.count { return $0.count > $1.count }
            return $0.term.localizedCaseInsensitiveCompare($1.term) == .orderedAscending
        }
    }

    static func replacementMap(
        for candidates: [GlossaryCandidate],
        preserving previous: [String: String]
    ) -> [String: String] {
        var previousByKey: [String: String] = [:]
        for (term, replacement) in previous {
            previousByKey[normalizedKey(term)] = replacement
        }
        var result: [String: String] = [:]
        for candidate in merged(candidates) {
            result[candidate.term] = previousByKey[normalizedKey(candidate.term)] ?? ""
        }
        return result
    }

    static func applying(_ replacements: [String: String], to text: String) -> String {
        let selected = replacements.compactMap { source, target -> (String, String)? in
            let source = source.trimmingCharacters(in: .whitespacesAndNewlines)
            let target = target.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !source.isEmpty, !target.isEmpty, source != target else { return nil }
            return (source, target)
        }.sorted { $0.0.count > $1.0.count }

        return selected.reduce(text) { result, replacement in
            let escaped = NSRegularExpression.escapedPattern(for: replacement.0)
            let pattern = "(?<![\\p{L}\\p{N}_])\(escaped)(?![\\p{L}\\p{N}_])"
            guard let expression = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]) else {
                return result
            }
            let range = NSRange(result.startIndex..<result.endIndex, in: result)
            return expression.stringByReplacingMatches(
                in: result,
                range: range,
                withTemplate: NSRegularExpression.escapedTemplate(for: replacement.1)
            )
        }
    }

    static func activeReplacementCount(_ replacements: [String: String]) -> Int {
        replacements.filter {
            !$0.key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                && !$0.value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                && $0.key != $0.value
        }.count
    }

    private static func normalizedKey(_ term: String) -> String {
        term.trimmingCharacters(in: .whitespacesAndNewlines)
            .folding(options: [.caseInsensitive, .widthInsensitive], locale: Locale(identifier: "en_US_POSIX"))
    }
}
