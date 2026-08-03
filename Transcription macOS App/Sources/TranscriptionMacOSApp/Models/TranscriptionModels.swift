import Foundation

enum TranscriptLanguage: String, CaseIterable, Identifiable, Codable {
    case german = "de"
    case english = "en"

    var id: String { rawValue }
    var title: String { self == .german ? "Deutsch" : "English" }
}

enum WhisperModel: String, CaseIterable, Identifiable, Codable {
    case turbo
    case medium
    case small

    var id: String { rawValue }
    var title: String {
        switch self {
        case .turbo: return "Turbo – beste Qualität"
        case .medium: return "Medium – ausgewogen"
        case .small: return "Small – schnell"
        }
    }
}

enum SpeakerRange: String, CaseIterable, Identifiable, Codable {
    case automatic = "auto"
    case exactlyTwo = "2-2"
    case twoToFour = "2-4"
    case fourToSix = "4-6"
    case fiveToEight = "5-8"
    case sixToTen = "6-10"

    var id: String { rawValue }
    var title: String {
        switch self {
        case .automatic: return "Automatisch"
        case .exactlyTwo: return "Genau 2"
        case .twoToFour: return "2–4"
        case .fourToSix: return "4–6"
        case .fiveToEight: return "5–8"
        case .sixToTen: return "6–10"
        }
    }
}

enum SeparationPreset: String, CaseIterable, Identifiable, Codable {
    case loose, normal, strict, veryStrict

    var id: String { rawValue }
    var threshold: Double {
        switch self {
        case .loose: return 0.70
        case .normal: return 0.50
        case .strict: return 0.35
        case .veryStrict: return 0.25
        }
    }
    var title: String {
        switch self {
        case .loose: return "Locker – ähnliche Stimmen zusammen"
        case .normal: return "Normal"
        case .strict: return "Streng – mehr Trennung"
        case .veryStrict: return "Sehr streng"
        }
    }
}

enum OutputFormat: String, CaseIterable, Identifiable, Codable {
    case markdown, vtt, txt, csv
    var id: String { rawValue }
    var title: String { rawValue == "markdown" ? "Markdown" : rawValue.uppercased() }
}

struct SelectedMediaFile: Identifiable, Hashable {
    let id = UUID()
    let url: URL
    var name: String { url.lastPathComponent }
    var folder: String { url.deletingLastPathComponent().path }
}

struct TranscriptionSettings {
    var language: TranscriptLanguage
    var model: WhisperModel
    var includeTimecodes: Bool
    var diarizationEnabled: Bool
    var speakerRange: SpeakerRange
    var separation: SeparationPreset
    var outputFormats: Set<OutputFormat>
    var outputFolder: URL?
}

struct LogEntry: Identifiable, Equatable {
    enum Kind { case info, success, error }
    let id = UUID()
    let timestamp: Date
    let message: String
    let kind: Kind
}

struct RunnerEvent: Decodable {
    let type: String
    let fileID: String?
    let sourcePath: String?
    let documentPath: String?
    let stage: String?
    let percent: Int?
    let message: String?
    let outputs: [String: String]?
    let speakerCount: Int?
    let segmentCount: Int?
    let succeeded: Int?
    let failed: Int?
    let candidates: [GlossaryCandidate]?

    enum CodingKeys: String, CodingKey {
        case type, stage, percent, message, outputs, succeeded, failed, candidates
        case fileID = "file_id"
        case sourcePath = "source_path"
        case documentPath = "document_path"
        case speakerCount = "speaker_count"
        case segmentCount = "segment_count"
    }
}

struct TranscriptionResult: Identifiable, Equatable {
    let id = UUID()
    let sourceURL: URL
    let documentURL: URL
    var outputs: [String: String]
    let speakerCount: Int
    let segmentCount: Int
}

struct TranscriptDocument: Codable, Identifiable {
    var id: String
    var sourcePath: String
    var sourceFile: String
    var language: String
    var model: String
    var device: String
    var engine: String
    var created: String
    var fpsTimecode: Int
    var timecodes: Bool
    var diarization: Bool
    var speakerModel: String?
    var speakerNames: [String: String]
    var speakerRegions: [SpeakerRegion]
    var segments: [TranscriptSegment]
    var outputs: [String: String]

    enum CodingKeys: String, CodingKey {
        case id, language, model, device, engine, created, timecodes, diarization, segments, outputs
        case sourcePath = "source_path"
        case sourceFile = "source_file"
        case fpsTimecode = "fps_timecode"
        case speakerModel = "speaker_model"
        case speakerNames = "speaker_names"
        case speakerRegions = "speaker_regions"
    }
}

struct SpeakerRegion: Codable, Hashable {
    var start: Double
    var end: Double
    var speaker: String
}

struct TranscriptSegment: Codable, Identifiable, Hashable {
    var id: String
    var start: Double
    var end: Double
    var speaker: String?
    var text: String
}

struct GlossaryCandidate: Codable, Identifiable, Hashable {
    var term: String
    var count: Int
    var kind: String
    var id: String { "\(term)-\(kind)" }
}
