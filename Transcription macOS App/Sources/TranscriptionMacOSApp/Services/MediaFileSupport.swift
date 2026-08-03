import Foundation
import UniformTypeIdentifiers

enum MediaFileSupport {
    static let audioExtensions: Set<String> = [
        "aac", "ac3", "aif", "aiff", "amr", "caf", "flac", "m4a", "m4b",
        "mp2", "mp3", "oga", "ogg", "opus", "wav", "wave", "wma",
    ]

    static let videoExtensions: Set<String> = [
        "3g2", "3gp", "asf", "avi", "flv", "m2ts", "m4v", "mkv", "mov",
        "mp4", "mpeg", "mpg", "mts", "ogv", "ts", "vob", "webm", "wmv",
    ]

    static var openPanelContentTypes: [UTType] {
        var seen: Set<String> = []
        let explicit = (audioExtensions.union(videoExtensions)).sorted().compactMap { UTType(filenameExtension: $0) }
        return ([UTType.audio, .movie, .mpeg4Movie, .quickTimeMovie] + explicit).filter {
            seen.insert($0.identifier).inserted
        }
    }

    static func isSupported(_ url: URL) -> Bool {
        guard url.isFileURL, !url.hasDirectoryPath else { return false }
        let fileExtension = url.pathExtension.lowercased()
        return audioExtensions.contains(fileExtension) || videoExtensions.contains(fileExtension)
    }

    static func isVideo(_ url: URL) -> Bool {
        videoExtensions.contains(url.pathExtension.lowercased())
    }
}
