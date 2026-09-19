import Foundation

enum PodcastDownloadError: LocalizedError {
    case noDestination
    case insufficientSpace
    case unavailableEpisode
    case invalidMedia(String)
    case ffmpegMissing
    case ffmpegFailed(String)
    case cancelled

    var errorDescription: String? {
        switch self {
        case .noDestination: return "Vor dem Download muss ein Zielordner gewählt werden."
        case .insufficientSpace: return "Im Zielordner ist nicht genügend freier Speicherplatz verfügbar."
        case .unavailableEpisode: return "Diese Folge besitzt keinen verwertbaren Titel oder Audio-Enclosure."
        case .invalidMedia(let reason): return "Die heruntergeladene Datei ist kein lesbares Audiomedium: \(reason)"
        case .ffmpegMissing: return "Das gebündelte ffmpeg wurde nicht gefunden."
        case .ffmpegFailed(let reason): return "ffmpeg konnte die Folge nicht als MP3 verarbeiten: \(reason)"
        case .cancelled: return "Download abgebrochen."
        }
    }
}

struct PodcastDownloadState: Identifiable, Equatable {
    let id: String
    var title: String
    var phase: String
    var progress: Double
    var error: String?
    var destination: URL?
}

@MainActor
final class PodcastDownloadCoordinator: ObservableObject {
    @Published private(set) var states: [PodcastDownloadState] = []
    @Published private(set) var isRunning = false
    private var cancellationRequested = false
    private var activeDownload: PodcastAudioDownload?
    private var activeProcess: Process?

    func cancel() {
        cancellationRequested = true
        activeDownload?.cancel()
        activeProcess?.terminate()
    }

    func download(feed: PodcastFeed, episodes: [PodcastEpisode], to directory: URL) async -> [SelectedMediaFile] {
        guard !isRunning else { return [] }
        isRunning = true
        cancellationRequested = false
        states = episodes.map { PodcastDownloadState(id: $0.id, title: $0.title ?? "Unbenannte Folge", phase: "Wartet", progress: 0, error: nil, destination: nil) }
        let accessed = directory.startAccessingSecurityScopedResource()
        defer {
            if accessed { directory.stopAccessingSecurityScopedResource() }
            activeDownload = nil
            activeProcess = nil
            isRunning = false
        }

        var results: [SelectedMediaFile] = []
        for episode in episodes {
            if cancellationRequested { break }
            do {
                let result = try await downloadOne(feed: feed, episode: episode, directory: directory)
                results.append(result)
            } catch {
                update(episode.id) {
                    $0.phase = error is CancellationError || self.cancellationRequested ? "Abgebrochen" : "Fehlgeschlagen"
                    $0.error = error.localizedDescription
                }
            }
        }
        return results
    }

    private func downloadOne(feed: PodcastFeed, episode: PodcastEpisode, directory: URL) async throws -> SelectedMediaFile {
        guard episode.unavailableReason == nil, let sourceURL = episode.audioURL, let title = cleanTitle(episode.title) else {
            throw PodcastDownloadError.unavailableEpisode
        }
        let values = try? directory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
        if let capacity = values?.volumeAvailableCapacityForImportantUsage, capacity < 100_000_000 {
            throw PodcastDownloadError.insufficientSpace
        }
        let work = FileManager.default.temporaryDirectory.appendingPathComponent("podcast-download-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: work) }
        let source = work.appendingPathComponent("source")
        update(episode.id) { $0.phase = "Wird geladen"; $0.progress = 0.01 }
        let download = PodcastAudioDownload(destination: source) { [weak self] progress in
            Task { @MainActor in self?.update(episode.id) { $0.progress = min(0.82, progress * 0.82) } }
        }
        activeDownload = download
        let response = try await download.start(url: sourceURL)
        activeDownload = nil
        if cancellationRequested { throw PodcastDownloadError.cancelled }

        let ffmpeg = try Self.ffmpegURL()
        let staging = work.appendingPathComponent("ready.mp3")
        let isMP3 = response.mimeType?.lowercased().contains("mpeg") == true
            || sourceURL.pathExtension.lowercased() == "mp3"
            || episode.enclosureType?.lowercased().contains("mpeg") == true
        update(episode.id) { $0.phase = isMP3 ? "MP3 wird geprüft" : "Wird in MP3 umgewandelt"; $0.progress = 0.85 }
        let process = Process()
        activeProcess = process
        let errorText = try await Self.prepareMP3(source: source, destination: staging, ffmpeg: ffmpeg, copyAudio: isMP3, process: process)
        activeProcess = nil
        guard errorText.isEmpty else { throw PodcastDownloadError.ffmpegFailed(errorText) }
        if cancellationRequested { throw PodcastDownloadError.cancelled }

        let finalURL = Self.availableDestination(
            in: directory,
            publishedAt: episode.publishedAt,
            showTitle: feed.showTitle,
            episodeTitle: title
        )
        try FileManager.default.moveItem(at: staging, to: finalURL)
        let metadata = PodcastMetadata(
            feedURL: feed.feedURL.absoluteString,
            podcastIndexFeedID: feed.podcastIndexFeedID,
            showTitle: feed.showTitle,
            episodeTitle: title,
            author: episode.author ?? feed.author,
            publisher: episode.publisher ?? feed.publisher,
            language: episode.language ?? feed.language,
            publishedAt: episode.publishedAt.map(Self.iso8601.string(from:)),
            downloadedAt: Self.iso8601.string(from: Date()),
            episodeNumber: episode.episodeNumber,
            seasonNumber: episode.seasonNumber,
            episodeType: episode.episodeType,
            guid: episode.guid,
            episodeURL: episode.episodeURL,
            audioURL: sourceURL.absoluteString,
            durationSeconds: episode.durationSeconds,
            explicit: episode.explicit,
            imageURL: episode.imageURL ?? feed.imageURL,
            categories: episode.categories.isEmpty ? feed.categories : episode.categories,
            showDescription: feed.description,
            episodeDescription: episode.description,
            rawShowDescription: feed.rawDescription,
            rawEpisodeDescription: episode.rawDescription
        )
        let selected = SelectedMediaFile(url: finalURL, podcastMetadata: metadata)
        PodcastDownloadRegistry.register(selected)
        update(episode.id) { $0.phase = "Abgeschlossen"; $0.progress = 1; $0.destination = finalURL }
        return selected
    }

    private func update(_ id: String, change: (inout PodcastDownloadState) -> Void) {
        guard let index = states.firstIndex(where: { $0.id == id }) else { return }
        change(&states[index])
    }

    static func sanitizedComponent(_ raw: String, maximumLength: Int = 100) -> String {
        var value = raw.replacingOccurrences(of: "[/:\\\\?%*|\"<>\\p{Cc}]", with: "–", options: .regularExpression)
        value = value.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            .trimmingCharacters(in: CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: ".")))
        value = truncatedUTF8(value, maximumBytes: maximumLength).trimmingCharacters(in: .whitespaces)
        return value.isEmpty ? "Unbenannt" : value
    }

    static func availableDestination(in directory: URL, publishedAt: Date?, showTitle: String, episodeTitle: String) -> URL {
        var parts: [String] = []
        if let publishedAt { parts.append(dayFormatter.string(from: publishedAt)) }
        parts.append(sanitizedComponent(showTitle, maximumLength: 80))
        parts.append(sanitizedComponent(episodeTitle, maximumLength: 120))
        let base = truncatedUTF8(parts.joined(separator: " – "), maximumBytes: 220).trimmingCharacters(in: .whitespaces)
        var candidate = directory.appendingPathComponent(base).appendingPathExtension("mp3")
        var counter = 2
        while FileManager.default.fileExists(atPath: candidate.path) {
            candidate = directory.appendingPathComponent("\(base)_\(counter)").appendingPathExtension("mp3")
            counter += 1
        }
        return candidate
    }

    private static func prepareMP3(source: URL, destination: URL, ffmpeg: URL, copyAudio: Bool, process: Process) async throws -> String {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let errorPipe = Pipe()
                process.executableURL = ffmpeg
                process.arguments = copyAudio
                    ? ["-nostdin", "-v", "error", "-i", source.path, "-map", "0:a:0", "-c:a", "copy", "-f", "mp3", destination.path]
                    : ["-nostdin", "-v", "error", "-i", source.path, "-map", "0:a:0", "-vn", "-c:a", "libmp3lame", "-q:a", "2", destination.path]
                process.standardOutput = Pipe()
                process.standardError = errorPipe
                do {
                    try process.run()
                    process.waitUntilExit()
                    let data = errorPipe.fileHandleForReading.readDataToEndOfFile()
                    let message = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                    continuation.resume(returning: process.terminationStatus == 0 ? "" : (message.isEmpty ? "Prozesscode \(process.terminationStatus)" : message))
                } catch { continuation.resume(throwing: error) }
            }
        }
    }

    private static func truncatedUTF8(_ raw: String, maximumBytes: Int) -> String {
        var value = raw
        while value.utf8.count > maximumBytes && !value.isEmpty { value.removeLast() }
        return value
    }

    private static func ffmpegURL() throws -> URL {
        let candidates = [
            Bundle.main.resourceURL?.appendingPathComponent("bin/ffmpeg"),
            URL(fileURLWithPath: "/opt/homebrew/bin/ffmpeg"),
            URL(fileURLWithPath: "/usr/local/bin/ffmpeg"),
            URL(fileURLWithPath: "/usr/bin/ffmpeg"),
        ].compactMap { $0 }
        guard let value = candidates.first(where: { FileManager.default.isExecutableFile(atPath: $0.path) }) else {
            throw PodcastDownloadError.ffmpegMissing
        }
        return value
    }

    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter(); formatter.locale = Locale(identifier: "en_US_POSIX"); formatter.dateFormat = "yyyy-MM-dd"; return formatter
    }()
    private static let iso8601: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter(); formatter.formatOptions = [.withInternetDateTime]; return formatter
    }()
}

private func cleanTitle(_ value: String?) -> String? {
    guard let value else { return nil }
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}

private final class PodcastAudioDownload: NSObject, URLSessionDownloadDelegate, @unchecked Sendable {
    private let destination: URL
    private let onProgress: @Sendable (Double) -> Void
    private var continuation: CheckedContinuation<HTTPURLResponse, Error>?
    private var session: URLSession?
    private var task: URLSessionDownloadTask?
    private var response: HTTPURLResponse?

    init(destination: URL, onProgress: @escaping @Sendable (Double) -> Void) {
        self.destination = destination
        self.onProgress = onProgress
    }

    func start(url: URL) async throws -> HTTPURLResponse {
        guard ["https", "http"].contains(url.scheme?.lowercased() ?? "") else { throw PodcastServiceError.insecureResource }
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                self.continuation = continuation
                let configuration = URLSessionConfiguration.ephemeral
                configuration.timeoutIntervalForResource = 60 * 60 * 4
                let session = URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
                self.session = session
                var request = URLRequest(url: url)
                request.setValue(PodcastUserAgent.value, forHTTPHeaderField: "User-Agent")
                let task = session.downloadTask(with: request)
                self.task = task
                task.resume()
            }
        } onCancel: { self.cancel() }
    }

    func cancel() { task?.cancel() }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData bytesWritten: Int64, totalBytesWritten: Int64, totalBytesExpectedToWrite: Int64) {
        if totalBytesExpectedToWrite > 0 { onProgress(Double(totalBytesWritten) / Double(totalBytesExpectedToWrite)) }
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        guard let response = downloadTask.response as? HTTPURLResponse,
              response.url?.scheme?.lowercased() == "https" else {
            continuation?.resume(throwing: PodcastServiceError.insecureResource); continuation = nil; return
        }
        guard (200..<300).contains(response.statusCode) else {
            continuation?.resume(throwing: PodcastServiceError.server(response.statusCode)); continuation = nil; return
        }
        self.response = response
        do {
            try FileManager.default.moveItem(at: location, to: destination)
        } catch {
            continuation?.resume(throwing: error); continuation = nil
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        defer { session.finishTasksAndInvalidate() }
        guard let continuation else { return }
        self.continuation = nil
        if let error { continuation.resume(throwing: error) }
        else if let response { continuation.resume(returning: response) }
        else { continuation.resume(throwing: PodcastDownloadError.invalidMedia("Leere Serverantwort")) }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(request.url.map(SecureHTTPClient.isAllowedRedirect(to:)) == true ? request : nil)
    }
}
