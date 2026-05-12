import Foundation

enum TranscriptLanguage: String, CaseIterable, Identifiable {
    case german = "de"
    case english = "en"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .german: return "Deutsch"
        case .english: return "English"
        }
    }
}

struct SelectedMediaFile: Identifiable, Hashable {
    let id = UUID()
    let url: URL

    var name: String { url.lastPathComponent }
    var folder: String { url.deletingLastPathComponent().path }
}

struct TranscriptionSettings {
    var language: TranscriptLanguage
    var modelName: String
    var includeTimecodes: Bool
    var bufferSeconds: Int
    var outputFolder: URL?
}

struct LogEntry: Identifiable, Equatable {
    enum Kind {
        case info
        case success
        case error
    }

    let id = UUID()
    let timestamp: Date
    let message: String
    let kind: Kind
}
