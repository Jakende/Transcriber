import Foundation

enum YouTubeDownloadError: LocalizedError {
    case invalidURL
    case toolMissing
    case denoMissing
    case ffmpegMissing
    case metadataInvalid
    case noMedia
    case insufficientSpace
    case processFailed(String)
    case cancelled

    var errorDescription: String? {
        switch self {
        case .invalidURL:
            return "Bitte einen gültigen HTTPS-Link zu einem einzelnen YouTube-Video eingeben."
        case .toolMissing:
            return "yt-dlp wurde nicht gefunden. Für das vollständige App-Bundle muss die YouTube-Laufzeit eingebettet sein."
        case .denoMissing:
            return "Deno wurde nicht gefunden. Die JavaScript-Laufzeit wird für den YouTube-Download benötigt."
        case .ffmpegMissing:
            return "Das gebündelte ffmpeg wurde nicht gefunden."
        case .metadataInvalid:
            return "Die YouTube-Videoinformationen konnten nicht gelesen werden."
        case .noMedia:
            return "yt-dlp hat keine verwertbare Mediendatei erzeugt."
        case .insufficientSpace:
            return "Im gewählten Downloadordner ist nicht genügend freier Speicherplatz verfügbar."
        case .processFailed(let detail):
            return detail.isEmpty ? "Der YouTube-Download ist fehlgeschlagen." : detail
        case .cancelled:
            return "YouTube-Download abgebrochen."
        }
    }
}

struct YouTubeVideoInfo: Equatable {
    let id: String
    let title: String
    let channel: String?
    let durationSeconds: Int?
    let uploadDate: Date?
    let webpageURL: URL
}

struct YouTubeDownloadResult: Equatable {
    let audioFile: SelectedMediaFile
    let videoURL: URL?
}

@MainActor
final class YouTubeDownloadCoordinator: ObservableObject {
    @Published private(set) var info: YouTubeVideoInfo?
    @Published private(set) var phase = ""
    @Published private(set) var progress = 0.0
    @Published private(set) var errorMessage: String?
    @Published private(set) var isInspecting = false
    @Published private(set) var isRunning = false

    private var activeProcess: Process?
    private var cancellationRequested = false

    func reset() {
        guard !isRunning && !isInspecting else { return }
        info = nil
        phase = ""
        progress = 0
        errorMessage = nil
    }

    func cancel() {
        cancellationRequested = true
        activeProcess?.terminate()
    }

    func inspect(_ rawURL: String) async {
        guard !isRunning && !isInspecting else { return }
        isInspecting = true
        errorMessage = nil
        phase = "Videoinformationen werden geladen …"
        defer { isInspecting = false }

        do {
            let url = try Self.validatedURL(rawURL)
            let tools = try Self.resolveTools()
            let process = Process()
            activeProcess = process
            let result = try await Self.capture(
                command: tools.ytDLP,
                arguments: tools.ytDLPPrefix + Self.commonArguments(tools: tools) + [
                    "--dump-single-json",
                    "--skip-download",
                    url.absoluteString,
                ],
                environment: tools.environment,
                process: process
            )
            activeProcess = nil
            guard result.status == 0 else { throw YouTubeDownloadError.processFailed(Self.conciseError(result.error)) }
            info = try Self.decodeInfo(result.output, fallbackURL: url)
            phase = "Bereit zum Download"
        } catch {
            activeProcess = nil
            info = nil
            phase = ""
            errorMessage = error.localizedDescription
        }
    }

    func download(
        rawURL: String,
        info suppliedInfo: YouTubeVideoInfo?,
        keepVideo: Bool,
        to directory: URL
    ) async -> YouTubeDownloadResult? {
        guard !isRunning && !isInspecting else { return nil }
        isRunning = true
        cancellationRequested = false
        errorMessage = nil
        progress = 0
        let accessed = directory.startAccessingSecurityScopedResource()
        defer {
            if accessed { directory.stopAccessingSecurityScopedResource() }
            activeProcess = nil
            isRunning = false
        }

        do {
            let url = try Self.validatedURL(rawURL)
            let tools = try Self.resolveTools()
            let videoInfo: YouTubeVideoInfo
            if let suppliedInfo, suppliedInfo.webpageURL == url {
                videoInfo = suppliedInfo
            } else {
                phase = "Videoinformationen werden geladen …"
                let metadataProcess = Process()
                activeProcess = metadataProcess
                let metadata = try await Self.capture(
                    command: tools.ytDLP,
                    arguments: tools.ytDLPPrefix + Self.commonArguments(tools: tools) + ["--dump-single-json", "--skip-download", url.absoluteString],
                    environment: tools.environment,
                    process: metadataProcess
                )
                guard metadata.status == 0 else { throw YouTubeDownloadError.processFailed(Self.conciseError(metadata.error)) }
                videoInfo = try Self.decodeInfo(metadata.output, fallbackURL: url)
                info = videoInfo
            }
            if cancellationRequested { throw YouTubeDownloadError.cancelled }

            let minimumCapacity: Int64 = keepVideo ? 500_000_000 : 100_000_000
            let values = try? directory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
            if let capacity = values?.volumeAvailableCapacityForImportantUsage, capacity < minimumCapacity {
                throw YouTubeDownloadError.insufficientSpace
            }

            let work = FileManager.default.temporaryDirectory
                .appendingPathComponent("youtube-download-\(UUID().uuidString)", isDirectory: true)
            try FileManager.default.createDirectory(at: work, withIntermediateDirectories: true)
            defer { try? FileManager.default.removeItem(at: work) }

            phase = keepVideo ? "Video und Audio werden geladen …" : "Audio wird geladen …"
            progress = 0.01
            let downloadProcess = Process()
            activeProcess = downloadProcess
            var arguments = tools.ytDLPPrefix + Self.commonArguments(tools: tools) + [
                "--newline",
                "--no-colors",
                "--progress-template", "download:__PROGRESS__%(progress._percent_str)s",
                "--output", work.appendingPathComponent("source.%(ext)s").path,
            ]
            if keepVideo {
                arguments += ["--format", "bestvideo*+bestaudio/best", "--merge-output-format", "mp4", "--remux-video", "mp4"]
            } else {
                arguments += ["--format", "bestaudio/best"]
            }
            arguments.append(url.absoluteString)
            let downloadResult = try await Self.stream(
                command: tools.ytDLP,
                arguments: arguments,
                environment: tools.environment,
                process: downloadProcess
            ) { [weak self] line in
                guard let value = Self.progressValue(line) else { return }
                Task { @MainActor in self?.progress = 0.02 + value * 0.78 }
            }
            activeProcess = nil
            guard downloadResult.status == 0 else {
                if cancellationRequested { throw YouTubeDownloadError.cancelled }
                throw YouTubeDownloadError.processFailed(Self.conciseError(downloadResult.output))
            }
            if cancellationRequested { throw YouTubeDownloadError.cancelled }

            guard let downloadedMedia = try Self.downloadedMedia(in: work) else { throw YouTubeDownloadError.noMedia }
            phase = "MP3 wird erstellt …"
            progress = 0.84
            let stagedAudio = work.appendingPathComponent("ready.mp3")
            let ffmpegProcess = Process()
            activeProcess = ffmpegProcess
            let conversion = try await Self.capture(
                command: tools.ffmpeg,
                arguments: [
                    "-nostdin", "-v", "error", "-i", downloadedMedia.path,
                    "-map", "0:a:0", "-vn", "-c:a", "libmp3lame", "-q:a", "2", stagedAudio.path,
                ],
                environment: tools.environment,
                process: ffmpegProcess
            )
            activeProcess = nil
            guard conversion.status == 0, FileManager.default.fileExists(atPath: stagedAudio.path) else {
                if cancellationRequested { throw YouTubeDownloadError.cancelled }
                throw YouTubeDownloadError.processFailed(Self.conciseError(conversion.error))
            }
            if cancellationRequested { throw YouTubeDownloadError.cancelled }

            phase = "Dateien werden gespeichert …"
            progress = 0.95
            let videoExtension = keepVideo ? downloadedMedia.pathExtension.lowercased() : nil
            let destinations = Self.availableDestinations(
                in: directory,
                info: videoInfo,
                videoExtension: videoExtension
            )
            var movedVideo: URL?
            do {
                if let videoURL = destinations.video {
                    try FileManager.default.moveItem(at: downloadedMedia, to: videoURL)
                    movedVideo = videoURL
                }
                try FileManager.default.moveItem(at: stagedAudio, to: destinations.audio)
            } catch {
                if let movedVideo { try? FileManager.default.removeItem(at: movedVideo) }
                throw error
            }

            phase = "Abgeschlossen"
            progress = 1
            return YouTubeDownloadResult(
                audioFile: SelectedMediaFile(url: destinations.audio),
                videoURL: destinations.video
            )
        } catch {
            phase = cancellationRequested ? "Abgebrochen" : "Fehlgeschlagen"
            errorMessage = cancellationRequested ? YouTubeDownloadError.cancelled.localizedDescription : error.localizedDescription
            return nil
        }
    }

    static func validatedURL(_ rawValue: String) throws -> URL {
        let value = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: value), url.scheme?.lowercased() == "https", let host = url.host?.lowercased() else {
            throw YouTubeDownloadError.invalidURL
        }
        let allowed = host == "youtu.be"
            || host == "youtube.com"
            || host.hasSuffix(".youtube.com")
            || host == "youtube-nocookie.com"
            || host.hasSuffix(".youtube-nocookie.com")
        let pathParts = url.path.split(separator: "/")
        let isShortLink = host == "youtu.be" && !pathParts.isEmpty
        let isWatchLink = url.path == "/watch"
            && URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.contains(where: { $0.name == "v" && !($0.value ?? "").isEmpty }) == true
        let isDirectVideoPath = ["shorts", "live", "embed"].contains(pathParts.first.map(String.init) ?? "")
            && pathParts.count >= 2
        guard allowed && (isShortLink || isWatchLink || isDirectVideoPath) else { throw YouTubeDownloadError.invalidURL }
        return url
    }

    static func decodeInfo(_ data: Data, fallbackURL: URL) throws -> YouTubeVideoInfo {
        let raw = try JSONDecoder().decode(YouTubeInfoPayload.self, from: data)
        let title = raw.title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard let id = raw.id, !id.isEmpty, !title.isEmpty else { throw YouTubeDownloadError.metadataInvalid }
        let webpageURL = raw.webpageURL.flatMap(URL.init(string:)) ?? fallbackURL
        return YouTubeVideoInfo(
            id: id,
            title: title,
            channel: raw.channel ?? raw.uploader,
            durationSeconds: raw.duration.map { Int($0.rounded()) },
            uploadDate: raw.uploadDate.flatMap(dayParser.date(from:)),
            webpageURL: webpageURL
        )
    }

    static func availableDestinations(
        in directory: URL,
        info: YouTubeVideoInfo,
        videoExtension: String?
    ) -> (audio: URL, video: URL?) {
        var parts: [String] = []
        if let uploadDate = info.uploadDate { parts.append(dayFormatter.string(from: uploadDate)) }
        parts.append("YouTube")
        parts.append(PodcastDownloadCoordinator.sanitizedComponent(info.title, maximumLength: 150))
        let joined = parts.joined(separator: " – ")
        let base = PodcastDownloadCoordinator.sanitizedComponent(joined, maximumLength: 220)
        var suffix = 1
        while true {
            let candidateBase = suffix == 1 ? base : "\(base)_\(suffix)"
            let audio = directory.appendingPathComponent(candidateBase).appendingPathExtension("mp3")
            let video = videoExtension.map { directory.appendingPathComponent(candidateBase).appendingPathExtension($0) }
            if !FileManager.default.fileExists(atPath: audio.path)
                && video.map({ !FileManager.default.fileExists(atPath: $0.path) }) != false {
                return (audio, video)
            }
            suffix += 1
        }
    }

    private static func commonArguments(tools: YouTubeTools) -> [String] {
        [
            "--no-config",
            "--no-playlist",
            "--ffmpeg-location", tools.ffmpeg.deletingLastPathComponent().path,
            "--js-runtimes", "deno:\(tools.deno.path)",
        ]
    }

    private static func resolveTools() throws -> YouTubeTools {
        let resources = Bundle.main.resourceURL
        let project = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        let bundledPython = resources?.appendingPathComponent("python-runtime/bin/python3")
        let cachedPython = project.appendingPathComponent(".bundle-cache/resources/python-runtime/bin/python3")
        let directYTDLP = [
            resources?.appendingPathComponent("bin/yt-dlp"),
            URL(fileURLWithPath: "/opt/homebrew/bin/yt-dlp"),
            URL(fileURLWithPath: "/usr/local/bin/yt-dlp"),
        ].compactMap { $0 }.first(where: executable)

        let ytDLP: URL
        let prefix: [String]
        if let python = [bundledPython, cachedPython].compactMap({ $0 }).first(where: executable), pythonCanImportYTDLP(python) {
            ytDLP = python
            prefix = ["-m", "yt_dlp"]
        } else if let directYTDLP {
            ytDLP = directYTDLP
            prefix = []
        } else {
            throw YouTubeDownloadError.toolMissing
        }

        guard let ffmpeg = [
            resources?.appendingPathComponent("bin/ffmpeg"),
            project.appendingPathComponent(".bundle-cache/resources/bin/ffmpeg"),
            URL(fileURLWithPath: "/opt/homebrew/bin/ffmpeg"),
            URL(fileURLWithPath: "/usr/local/bin/ffmpeg"),
        ].compactMap({ $0 }).first(where: executable) else { throw YouTubeDownloadError.ffmpegMissing }
        guard let deno = [
            resources?.appendingPathComponent("bin/deno"),
            project.appendingPathComponent(".bundle-cache/resources/bin/deno"),
            URL(fileURLWithPath: "/opt/homebrew/bin/deno"),
            URL(fileURLWithPath: "/usr/local/bin/deno"),
        ].compactMap({ $0 }).first(where: executable) else { throw YouTubeDownloadError.denoMissing }

        var environment = ProcessInfo.processInfo.environment
        environment["PATH"] = "\(ffmpeg.deletingLastPathComponent().path):\(deno.deletingLastPathComponent().path):/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        return YouTubeTools(ytDLP: ytDLP, ytDLPPrefix: prefix, ffmpeg: ffmpeg, deno: deno, environment: environment)
    }

    private static func executable(_ url: URL) -> Bool {
        FileManager.default.isExecutableFile(atPath: url.path)
    }

    private static func pythonCanImportYTDLP(_ python: URL) -> Bool {
        let process = Process()
        process.executableURL = python
        process.arguments = ["-c", "import yt_dlp"]
        var environment = ProcessInfo.processInfo.environment
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        process.environment = environment
        process.standardOutput = Pipe()
        process.standardError = Pipe()
        do {
            try process.run()
            process.waitUntilExit()
            return process.terminationStatus == 0
        } catch {
            return false
        }
    }

    private static func downloadedMedia(in directory: URL) throws -> URL? {
        let ignored = Set(["part", "ytdl", "json", "description", "jpg", "jpeg", "png", "webp"])
        return try FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.isRegularFileKey],
            options: [.skipsHiddenFiles]
        ).first { !ignored.contains($0.pathExtension.lowercased()) && $0.lastPathComponent != "ready.mp3" }
    }

    nonisolated private static func progressValue(_ line: String) -> Double? {
        guard let range = line.range(of: #"__PROGRESS__\s*([0-9]+(?:\.[0-9]+)?)%"#, options: .regularExpression) else { return nil }
        let matched = String(line[range])
        let number = matched.replacingOccurrences(of: "__PROGRESS__", with: "")
            .replacingOccurrences(of: "%", with: "")
            .trimmingCharacters(in: .whitespaces)
        return Double(number).map { min(1, max(0, $0 / 100)) }
    }

    private static func capture(
        command: URL,
        arguments: [String],
        environment: [String: String],
        process: Process
    ) async throws -> (status: Int32, output: Data, error: String) {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let outputPipe = Pipe()
                let errorPipe = Pipe()
                process.executableURL = command
                process.arguments = arguments
                process.environment = environment
                process.standardOutput = outputPipe
                process.standardError = errorPipe
                do {
                    try process.run()
                    let output = YouTubeLockedData()
                    let errorOutput = YouTubeLockedData()
                    let readers = DispatchGroup()
                    readers.enter()
                    DispatchQueue.global(qos: .userInitiated).async {
                        output.set(outputPipe.fileHandleForReading.readDataToEndOfFile())
                        readers.leave()
                    }
                    readers.enter()
                    DispatchQueue.global(qos: .userInitiated).async {
                        errorOutput.set(errorPipe.fileHandleForReading.readDataToEndOfFile())
                        readers.leave()
                    }
                    process.waitUntilExit()
                    readers.wait()
                    let error = String(data: errorOutput.value, encoding: .utf8) ?? ""
                    continuation.resume(returning: (process.terminationStatus, output.value, error))
                } catch {
                    continuation.resume(throwing: error)
                }
            }
        }
    }

    private static func stream(
        command: URL,
        arguments: [String],
        environment: [String: String],
        process: Process,
        onLine: @escaping @Sendable (String) -> Void
    ) async throws -> (status: Int32, output: String) {
        let pipe = Pipe()
        let collector = YouTubeProcessLineCollector(onLine: onLine)
        pipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            if data.isEmpty { collector.finish() } else { collector.append(data) }
        }
        process.executableURL = command
        process.arguments = arguments
        process.environment = environment
        process.standardOutput = pipe
        process.standardError = pipe
        do {
            try process.run()
            await withCheckedContinuation { continuation in
                process.terminationHandler = { _ in continuation.resume() }
            }
            pipe.fileHandleForReading.readabilityHandler = nil
            collector.finish()
            return (process.terminationStatus, collector.output)
        } catch {
            pipe.fileHandleForReading.readabilityHandler = nil
            collector.finish()
            throw error
        }
    }

    private static func conciseError(_ raw: String) -> String {
        let lines = raw.split(whereSeparator: \.isNewline).suffix(8)
        return lines.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static let dayParser: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyyMMdd"
        return formatter
    }()

    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }()
}

private struct YouTubeInfoPayload: Decodable {
    let id: String?
    let title: String?
    let channel: String?
    let uploader: String?
    let duration: Double?
    let uploadDate: String?
    let webpageURL: String?

    enum CodingKeys: String, CodingKey {
        case id, title, channel, uploader, duration
        case uploadDate = "upload_date"
        case webpageURL = "webpage_url"
    }
}

private struct YouTubeTools {
    let ytDLP: URL
    let ytDLPPrefix: [String]
    let ffmpeg: URL
    let deno: URL
    let environment: [String: String]
}

private final class YouTubeProcessLineCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = Data()
    private var lines: [String] = []
    private var finished = false
    private let onLine: @Sendable (String) -> Void

    init(onLine: @escaping @Sendable (String) -> Void) {
        self.onLine = onLine
    }

    var output: String {
        lock.lock()
        defer { lock.unlock() }
        return lines.suffix(80).joined(separator: "\n")
    }

    func append(_ data: Data) {
        lock.lock()
        guard !finished else { lock.unlock(); return }
        buffer.append(data)
        let extracted = extractLines()
        lock.unlock()
        extracted.forEach(onLine)
    }

    func finish() {
        lock.lock()
        guard !finished else { lock.unlock(); return }
        finished = true
        var extracted = extractLines()
        if !buffer.isEmpty, let tail = String(data: buffer, encoding: .utf8) {
            extracted.append(tail)
            lines.append(tail)
        }
        buffer.removeAll()
        lock.unlock()
        extracted.forEach(onLine)
    }

    private func extractLines() -> [String] {
        var extracted: [String] = []
        while let newline = buffer.firstIndex(of: 0x0A) {
            let data = buffer[..<newline]
            buffer.removeSubrange(...newline)
            let line = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .newlines)
            lines.append(line)
            if lines.count > 100 { lines.removeFirst(lines.count - 100) }
            extracted.append(line)
        }
        return extracted
    }
}

private final class YouTubeLockedData: @unchecked Sendable {
    private let lock = NSLock()
    private var stored = Data()

    var value: Data {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }

    func set(_ data: Data) {
        lock.lock()
        stored = data
        lock.unlock()
    }
}
