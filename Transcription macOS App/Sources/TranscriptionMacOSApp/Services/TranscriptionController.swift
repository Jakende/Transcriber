import AppKit
import Foundation

@MainActor
final class TranscriptionController: ObservableObject {
    @Published var files: [SelectedMediaFile] = []
    @Published var results: [TranscriptionResult] = []
    @Published var logEntries = [LogEntry(timestamp: Date(), message: "Bereit.", kind: .info)]
    @Published var progressLine = "Bereit"
    @Published var estimateLine = ""
    @Published var progressValue = 0.0
    @Published var isRunning = false

    let runner = PythonTranscriptionRunner()
    private var activeFiles: [SelectedMediaFile] = []
    private var startedAt: Date?

    func addFiles(_ urls: [URL]) {
        var seen = Set(files.map { $0.url.standardizedFileURL })
        var additions: [SelectedMediaFile] = []
        var rejected = 0
        for rawURL in urls {
            let url = rawURL.standardizedFileURL
            guard MediaFileSupport.isSupported(url) else {
                rejected += 1
                continue
            }
            guard seen.insert(url).inserted else { continue }
            additions.append(SelectedMediaFile(url: url))
        }
        files.append(contentsOf: additions)
        if !additions.isEmpty {
            appendLog("\(additions.count) Datei\(additions.count == 1 ? "" : "en") hinzugefügt.", kind: .info)
        }
        if rejected > 0 {
            appendLog("\(rejected) nicht unterstützte Datei\(rejected == 1 ? "" : "en") übersprungen.", kind: .error)
        }
    }

    func remove(_ file: SelectedMediaFile) { files.removeAll { $0.id == file.id } }

    func start(settings: TranscriptionSettings) {
        guard !files.isEmpty, !settings.outputFormats.isEmpty else { return }
        activeFiles = files
        results = []
        progressValue = 0
        estimateLine = ""
        startedAt = Date()
        progressLine = "Bereite Stapel vor …"
        isRunning = true
        appendLog("Transkription gestartet.", kind: .info)
        Task {
            do {
                try await runner.run(files: activeFiles, settings: settings) { [weak self] event in
                    self?.handle(event)
                } onDiagnostic: { [weak self] line in
                    self?.appendLog(line, kind: .info)
                }
                if isRunning {
                    progressValue = 1
                    progressLine = "Abgeschlossen"
                    appendLog("Stapel abgeschlossen.", kind: .success)
                }
            } catch {
                if isRunning {
                    progressLine = "Fehlgeschlagen"
                    appendLog(error.localizedDescription, kind: .error)
                }
            }
            isRunning = false
        }
    }

    func cancel() {
        guard isRunning else { return }
        runner.cancel()
        isRunning = false
        progressLine = "Abgebrochen"
        estimateLine = ""
        appendLog("Verarbeitung abgebrochen; fertige Ergebnisse bleiben erhalten.", kind: .error)
    }

    func importVTT(vttURL: URL, audioURL: URL?) throws -> TranscriptionResult {
        let document = try VTTParser.parse(vttURL: vttURL, audioURL: audioURL)
        let documentURL = try runner.store(document: document)
        let result = TranscriptionResult(
            sourceURL: audioURL ?? vttURL,
            documentURL: documentURL,
            outputs: [:],
            speakerCount: document.speakerNames.count,
            segmentCount: document.segments.count
        )
        results.append(result)
        appendLog("VTT importiert: \(vttURL.lastPathComponent)", kind: .success)
        return result
    }

    func reveal(result: TranscriptionResult) {
        if let path = result.outputs.values.first {
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
        } else {
            NSWorkspace.shared.activateFileViewerSelecting([result.sourceURL])
        }
    }

    func updateResult(_ result: TranscriptionResult, outputs: [String: String]) {
        guard let index = results.firstIndex(where: { $0.id == result.id }) else { return }
        results[index].outputs = outputs
    }

    func clearLog() {
        logEntries = [LogEntry(timestamp: Date(), message: "Bereit.", kind: .info)]
        if !isRunning { progressLine = "Bereit" }
    }

    func appendLog(_ message: String, kind: LogEntry.Kind) {
        let clean = message.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else { return }
        logEntries.append(LogEntry(timestamp: Date(), message: clean, kind: kind))
    }

    private func handle(_ event: RunnerEvent) {
        switch event.type {
        case "progress":
            let index = fileIndex(from: event.fileID)
            progressValue = (Double(index) + Double(event.percent ?? 0) / 100) / Double(max(1, activeFiles.count))
            progressLine = "Datei \(index + 1) von \(activeFiles.count): \(event.message ?? event.stage ?? "Verarbeitung")"
            if let startedAt, progressValue > 0.02, progressValue < 1 {
                let elapsed = Date().timeIntervalSince(startedAt)
                let remaining = max(0, elapsed / progressValue - elapsed)
                estimateLine = "Verstrichen \(duration(elapsed)) · etwa \(duration(remaining)) verbleibend"
            }
        case "result":
            guard let source = event.sourcePath, let document = event.documentPath else { return }
            let result = TranscriptionResult(
                sourceURL: URL(fileURLWithPath: source),
                documentURL: URL(fileURLWithPath: document),
                outputs: event.outputs ?? [:],
                speakerCount: event.speakerCount ?? 0,
                segmentCount: event.segmentCount ?? 0
            )
            results.append(result)
            appendLog("Gespeichert: \(result.sourceURL.lastPathComponent)", kind: .success)
        case "file_error", "fatal_error":
            appendLog(event.message ?? "Unbekannter Fehler", kind: .error)
        case "cancelled":
            progressLine = "Abgebrochen"
        case "batch_complete":
            progressLine = "\(event.succeeded ?? 0) abgeschlossen, \(event.failed ?? 0) fehlgeschlagen"
            estimateLine = ""
        default: break
        }
    }

    private func fileIndex(from fileID: String?) -> Int {
        guard let fileID, let value = Int(fileID.replacingOccurrences(of: "file-", with: "")) else { return 0 }
        return value
    }

    private func duration(_ seconds: TimeInterval) -> String {
        let value = max(0, Int(seconds.rounded()))
        return value >= 3600 ? String(format: "%d:%02d:%02d", value / 3600, (value % 3600) / 60, value % 60) : String(format: "%02d:%02d", value / 60, value % 60)
    }
}
