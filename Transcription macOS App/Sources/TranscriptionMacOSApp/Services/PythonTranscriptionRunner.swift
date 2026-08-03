import Foundation

enum TranscriptionRunnerError: LocalizedError {
    case helperNotFound
    case compatiblePythonNotFound([String])
    case processAlreadyRunning
    case processFailed(Int32)
    case invalidDocument

    var errorDescription: String? {
        switch self {
        case .helperNotFound:
            return "Die eingebettete Transkriptionshilfe wurde nicht gefunden."
        case .compatiblePythonNotFound(let candidates):
            return "Keine kompatible Python-Laufzeit gefunden. Geprüft:\n" + candidates.joined(separator: "\n")
        case .processAlreadyRunning:
            return "Es läuft bereits eine Transkription oder Begriffsanalyse."
        case .processFailed(let code):
            return "Der Transkriptionsprozess wurde mit Code \(code) beendet."
        case .invalidDocument:
            return "Das Transkriptdokument konnte nicht gelesen werden."
        }
    }
}

@MainActor
final class PythonTranscriptionRunner {
    private var activeProcess: Process?
    private var cancellationRequested = false
    private let dependencyCheckCode = "import torch, numpy, soundfile"

    var isRunning: Bool { activeProcess != nil }

    func run(
        files: [SelectedMediaFile],
        settings: TranscriptionSettings,
        onEvent: @escaping @MainActor (RunnerEvent) -> Void,
        onDiagnostic: @escaping @MainActor (String) -> Void
    ) async throws {
        var arguments = ["--mode", "transcribe", "--language", settings.language.rawValue, "--model", settings.model.rawValue]
        arguments += ["--speaker-range", settings.speakerRange.rawValue, "--cluster-threshold", String(settings.separation.threshold)]
        if settings.includeTimecodes { arguments.append("--timecodes") }
        if settings.diarizationEnabled { arguments.append("--diarize") }
        for format in settings.outputFormats.sorted(by: { $0.rawValue < $1.rawValue }) {
            arguments += ["--format", format.rawValue]
        }
        if let outputFolder = settings.outputFolder {
            arguments += ["--output-dir", outputFolder.path]
        }
        for file in files { arguments += ["--file", file.url.path] }
        try await execute(arguments: arguments, onEvent: onEvent, onDiagnostic: onDiagnostic)
    }

    func extractGlossary(documentURL: URL) async throws -> [GlossaryCandidate] {
        var candidates: [GlossaryCandidate] = []
        try await execute(
            arguments: ["--mode", "glossary", "--document", documentURL.path],
            onEvent: { event in candidates = event.candidates ?? candidates },
            onDiagnostic: { _ in }
        )
        return candidates
    }

    func extractGlossary(document: TranscriptDocument) async throws -> [GlossaryCandidate] {
        try await extractGlossary(documentURL: persistTemporaryDocument(document))
    }

    func save(document: TranscriptDocument, at documentURL: URL) async throws -> [String: String] {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        let data = try encoder.encode(document)
        try data.write(to: documentURL, options: .atomic)
        var outputs: [String: String] = document.outputs
        try await execute(
            arguments: ["--mode", "render", "--document", documentURL.path],
            onEvent: { event in outputs = event.outputs ?? outputs },
            onDiagnostic: { _ in }
        )
        return outputs
    }

    func export(document: TranscriptDocument, to folder: URL, formats: Set<OutputFormat>) async throws -> [String: String] {
        let stateURL = try persistTemporaryDocument(document)
        var arguments = ["--mode", "render", "--document", stateURL.path, "--output-dir", folder.path]
        for format in formats.sorted(by: { $0.rawValue < $1.rawValue }) {
            arguments += ["--format", format.rawValue]
        }
        var outputs: [String: String] = [:]
        try await execute(
            arguments: arguments,
            onEvent: { event in outputs = event.outputs ?? outputs },
            onDiagnostic: { _ in }
        )
        return outputs
    }

    func loadDocument(at url: URL) throws -> TranscriptDocument {
        let data = try Data(contentsOf: url)
        do { return try JSONDecoder().decode(TranscriptDocument.self, from: data) }
        catch { throw TranscriptionRunnerError.invalidDocument }
    }

    func store(document: TranscriptDocument) throws -> URL {
        try persistTemporaryDocument(document)
    }

    func cancel() {
        cancellationRequested = true
        activeProcess?.terminate()
    }

    private func execute(
        arguments: [String],
        onEvent: @escaping @MainActor (RunnerEvent) -> Void,
        onDiagnostic: @escaping @MainActor (String) -> Void
    ) async throws {
        guard activeProcess == nil else { throw TranscriptionRunnerError.processAlreadyRunning }
        guard let helperURL = helperScriptURL() else { throw TranscriptionRunnerError.helperNotFound }
        let python = try resolvePythonExecutable()
        let resourceRoot = helperURL.deletingLastPathComponent()
        let stateDirectory = try applicationSupportDirectory().appendingPathComponent("Jobs", isDirectory: true)
        try FileManager.default.createDirectory(at: stateDirectory, withIntermediateDirectories: true)

        let process = Process()
        process.executableURL = URL(fileURLWithPath: python)
        process.arguments = [helperURL.path] + arguments + ["--resource-root", resourceRoot.path, "--state-dir", stateDirectory.path]
        process.environment = processEnvironment(resourceRoot: resourceRoot)
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        cancellationRequested = false
        activeProcess = process

        let collector = ProcessLineCollector { line in
            if let data = line.data(using: .utf8), let event = try? JSONDecoder().decode(RunnerEvent.self, from: data) {
                Task { @MainActor in onEvent(event) }
            } else if !line.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Task { @MainActor in onDiagnostic(line) }
            }
        }
        pipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            if data.isEmpty { collector.finish() } else { collector.append(data) }
        }

        do {
            try process.run()
            await withCheckedContinuation { continuation in
                process.terminationHandler = { _ in continuation.resume() }
            }
            pipe.fileHandleForReading.readabilityHandler = nil
            collector.finish()
            await Task.yield()
            activeProcess = nil
            if process.terminationStatus != 0 && !cancellationRequested {
                throw TranscriptionRunnerError.processFailed(process.terminationStatus)
            }
        } catch {
            pipe.fileHandleForReading.readabilityHandler = nil
            activeProcess = nil
            throw error
        }
    }

    private func persistTemporaryDocument(_ document: TranscriptDocument) throws -> URL {
        let directory = try applicationSupportDirectory().appendingPathComponent("Jobs", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("\(document.id).transcript.json")
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        try encoder.encode(document).write(to: url, options: .atomic)
        return url
    }

    private func applicationSupportDirectory() throws -> URL {
        let base = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        return base.appendingPathComponent("Transcription macOS", isDirectory: true)
    }

    private func resolvePythonExecutable() throws -> String {
        let candidates = pythonCandidates()
        var checked: [String] = []
        for candidate in candidates {
            let path = candidate.standardizedFileURL.path
            guard FileManager.default.isExecutableFile(atPath: path) else {
                checked.append("\(path) (nicht ausführbar)")
                continue
            }
            if pythonSupportsDependencies(path) { return path }
            checked.append("\(path) (Python-Abhängigkeiten fehlen)")
        }
        throw TranscriptionRunnerError.compatiblePythonNotFound(checked)
    }

    private func pythonCandidates() -> [URL] {
        let resources = Bundle.main.resourceURL
        let project = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        return [
            resources?.appendingPathComponent("venv/bin/python"),
            resources?.appendingPathComponent("python-runtime/bin/python3"),
            project.appendingPathComponent(".venv/bin/python3"),
            project.deletingLastPathComponent().appendingPathComponent("Transcription macOS/.venv/bin/python3"),
            URL(fileURLWithPath: "/opt/homebrew/bin/python3"),
            URL(fileURLWithPath: "/usr/local/bin/python3"),
        ].compactMap { $0 }
    }

    private func pythonSupportsDependencies(_ path: String) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = ["-c", dependencyCheckCode]
        process.standardOutput = Pipe()
        process.standardError = Pipe()
        do {
            try process.run()
            process.waitUntilExit()
            return process.terminationStatus == 0
        } catch { return false }
    }

    private func processEnvironment(resourceRoot: URL) -> [String: String] {
        var environment = ProcessInfo.processInfo.environment
        environment["PATH"] = "\(resourceRoot.appendingPathComponent("bin").path):/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        environment["DYLD_LIBRARY_PATH"] = resourceRoot.appendingPathComponent("lib").path
        environment["PYTORCH_ENABLE_MPS_FALLBACK"] = "1"
        environment["PYTHONUNBUFFERED"] = "1"
        environment["PYTHONDONTWRITEBYTECODE"] = "1"
        environment["HF_HUB_OFFLINE"] = "1"
        environment["LOKY_MAX_CPU_COUNT"] = String(ProcessInfo.processInfo.activeProcessorCount)
        return environment
    }

    private func helperScriptURL() -> URL? {
        if let resource = Bundle.main.url(forResource: "transcribe_bulk", withExtension: "py") { return resource }
        if let resource = Bundle.module.url(forResource: "transcribe_bulk", withExtension: "py") { return resource }
        let development = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Resources/transcribe_bulk.py")
        return FileManager.default.fileExists(atPath: development.path) ? development : nil
    }
}

private final class ProcessLineCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var buffer = Data()
    private var finished = false
    private let onLine: (String) -> Void

    init(onLine: @escaping (String) -> Void) { self.onLine = onLine }

    func append(_ data: Data) {
        lock.lock()
        guard !finished else { lock.unlock(); return }
        buffer.append(data)
        let lines = drainCompleteLines()
        lock.unlock()
        lines.forEach(onLine)
    }

    func finish() {
        lock.lock()
        guard !finished else { lock.unlock(); return }
        finished = true
        let remaining = String(data: buffer, encoding: .utf8)?.trimmingCharacters(in: .newlines)
        buffer.removeAll()
        lock.unlock()
        if let remaining, !remaining.isEmpty { onLine(remaining) }
    }

    private func drainCompleteLines() -> [String] {
        var result: [String] = []
        while let newline = buffer.firstIndex(of: 0x0A) {
            let lineData = buffer[..<newline]
            buffer.removeSubrange(...newline)
            if let line = String(data: lineData, encoding: .utf8) { result.append(line) }
        }
        return result
    }
}
