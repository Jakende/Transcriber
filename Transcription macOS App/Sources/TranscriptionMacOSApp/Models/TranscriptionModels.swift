import Foundation

enum TranscriptLanguage: String, CaseIterable, Identifiable, Codable {
    case mixed = "auto"
    case german = "de"
    case english = "en"

    var id: String { rawValue }
    var title: String {
        switch self {
        case .mixed: return "Deutsch + Englisch (automatisch)"
        case .german: return "Deutsch"
        case .english: return "English"
        }
    }
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
        case .loose: return 0.78
        case .normal: return 0.60
        case .strict: return 0.45
        case .veryStrict: return 0.30
        }
    }
    var title: String {
        switch self {
        case .loose: return "Locker – ähnliche Stimmen zusammen"
        case .normal: return "Normal – ähnliche Stimmen eher zusammen"
        case .strict: return "Streng – mehr Trennung"
        case .veryStrict: return "Sehr streng"
        }
    }
}

enum OutputFormat: String, CaseIterable, Identifiable, Codable {
    case markdown, vtt, srt, txt, csv
    var id: String { rawValue }
    var title: String { rawValue == "markdown" ? "Markdown" : rawValue.uppercased() }
}

struct PodcastMetadata: Codable, Hashable {
    var feedURL: String
    var podcastIndexFeedID: Int?
    var showTitle: String
    var episodeTitle: String
    var author: String?
    var publisher: String?
    var language: String?
    var publishedAt: String?
    var downloadedAt: String
    var episodeNumber: Int?
    var seasonNumber: Int?
    var episodeType: String?
    var guid: String?
    var episodeURL: String?
    var audioURL: String
    var durationSeconds: Int?
    var explicit: Bool?
    var imageURL: String?
    var categories: [String]
    var showDescription: String?
    var episodeDescription: String?
    var rawShowDescription: String? = nil
    var rawEpisodeDescription: String? = nil

    enum CodingKeys: String, CodingKey {
        case author, publisher, language, guid, explicit, categories
        case feedURL = "feed_url"
        case podcastIndexFeedID = "podcast_index_feed_id"
        case showTitle = "show_title"
        case episodeTitle = "episode_title"
        case publishedAt = "published_at"
        case downloadedAt = "downloaded_at"
        case episodeNumber = "episode_number"
        case seasonNumber = "season_number"
        case episodeType = "episode_type"
        case episodeURL = "episode_url"
        case audioURL = "audio_url"
        case durationSeconds = "duration_seconds"
        case imageURL = "image_url"
        case showDescription = "show_description"
        case episodeDescription = "episode_description"
        case rawShowDescription = "raw_show_description"
        case rawEpisodeDescription = "raw_episode_description"
    }
}

struct SelectedMediaFile: Identifiable, Hashable, Codable {
    let id: UUID
    let url: URL
    var podcastMetadata: PodcastMetadata?

    init(id: UUID = UUID(), url: URL, podcastMetadata: PodcastMetadata? = nil) {
        self.id = id
        self.url = url
        self.podcastMetadata = podcastMetadata
    }

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
    var id: String { documentURL.standardizedFileURL.path }
    let sourceURL: URL
    let documentURL: URL
    var outputs: [String: String]
    var speakerCount: Int
    var segmentCount: Int
}

struct TranscriptDocument: Codable, Identifiable, Equatable {
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
    var podcast: PodcastMetadata? = nil

    enum CodingKeys: String, CodingKey {
        case id, language, model, device, engine, created, timecodes, diarization, segments, outputs, podcast
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
    var language: String? = nil
}

struct GlossaryCandidate: Codable, Identifiable, Hashable {
    var term: String
    var count: Int
    var kind: String
    var id: String { "\(term)-\(kind)" }
}
