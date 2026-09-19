import Foundation

enum VTTParserError: LocalizedError {
    case unreadable
    case noCues

    var errorDescription: String? {
        switch self {
        case .unreadable: return "Die VTT-Datei konnte nicht gelesen werden."
        case .noCues: return "Die VTT-Datei enthält keine erkennbaren Untertitel."
        }
    }
}

enum VTTParser {
    static func parse(vttURL: URL, audioURL: URL?) throws -> TranscriptDocument {
        guard let content = try? String(contentsOf: vttURL, encoding: .utf8) else { throw VTTParserError.unreadable }
        let normalized = content.replacingOccurrences(of: "\r\n", with: "\n")
        let blocks = normalized.components(separatedBy: "\n\n")
        var segments: [TranscriptSegment] = []
        var names: [String: String] = [:]
        var labelForName: [String: String] = [:]

        for block in blocks {
            let lines = block.components(separatedBy: .newlines).filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
            guard let timingIndex = lines.firstIndex(where: { $0.contains("-->") }) else { continue }
            let timing = lines[timingIndex].components(separatedBy: "-->")
            guard timing.count == 2,
                  let start = parseTime(timing[0]),
                  let end = parseTime(timing[1].trimmingCharacters(in: .whitespaces).components(separatedBy: " ").first ?? timing[1]) else { continue }
            var text = lines.dropFirst(timingIndex + 1).joined(separator: " ").trimmingCharacters(in: .whitespaces)
            guard !text.isEmpty else { continue }
            var speaker: String?
            if let colon = text.firstIndex(of: ":") {
                let candidate = String(text[..<colon]).trimmingCharacters(in: .whitespaces)
                if !candidate.isEmpty && candidate.count <= 60 && !candidate.contains(".") {
                    speaker = labelForName[candidate]
                    if speaker == nil {
                        speaker = String(format: "SPEAKER_%02d", labelForName.count)
                        labelForName[candidate] = speaker
                        names[speaker!] = candidate
                    }
                    text = String(text[text.index(after: colon)...]).trimmingCharacters(in: .whitespaces)
                }
            }
            segments.append(TranscriptSegment(id: UUID().uuidString, start: start, end: end, speaker: speaker, text: text))
        }
        guard !segments.isEmpty else { throw VTTParserError.noCues }
        let source = audioURL ?? vttURL
        return TranscriptDocument(
            id: UUID().uuidString,
            sourcePath: source.path,
            sourceFile: vttURL.deletingPathExtension().lastPathComponent + "_bearbeitet.vtt",
            language: "de",
            model: "importiert",
            device: "nicht zutreffend",
            engine: "VTT-Import",
            created: Self.timestamp.string(from: Date()),
            fpsTimecode: 25,
            timecodes: true,
            diarization: !names.isEmpty,
            speakerModel: nil,
            speakerNames: names,
            speakerRegions: [],
            segments: segments,
            outputs: [:]
        )
    }

    private static func parseTime(_ raw: String) -> Double? {
        let value = raw.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: ",", with: ".")
        let parts = value.components(separatedBy: ":")
        guard parts.count >= 2 else { return nil }
        if parts.count == 3, let hours = Double(parts[0]), let minutes = Double(parts[1]), let seconds = Double(parts[2]) {
            return hours * 3600 + minutes * 60 + seconds
        }
        if let minutes = Double(parts[0]), let seconds = Double(parts[1]) { return minutes * 60 + seconds }
        return nil
    }

    private static let timestamp: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH:mm"
        return formatter
    }()
}
