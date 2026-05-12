import Foundation

enum TranscriptionRunnerError: LocalizedError {
    case helperNotFound
    case compatiblePythonNotFound([String])
    case pythonFailed(Int32)

    var errorDescription: String? {
        switch self {
        case .helperNotFound:
            return "Python helper transcribe_bulk.py was not found in the app resources."
        case .compatiblePythonNotFound(let candidates):
            return """
            No Python installation with torch and openai-whisper was found.
            Checked:
            \(candidates.joined(separator: "\n"))

            Create a virtual environment in "Transcription macOS App/.venv" or "Transcription macOS/.venv", then install:
            pip install torch openai-whisper
            """
        case .pythonFailed(let code):
            return "Python transcription process exited with code \(code)."
        }
    }
}

final class PythonTranscriptionRunner {
    private let dependencyCheckCode = "import whisper, torch"

    func run(files: [SelectedMediaFile], settings: TranscriptionSettings, onLine: @escaping @MainActor (String) -> Void) async throws {
        guard let helperURL = helperScriptURL() else {
            throw TranscriptionRunnerError.helperNotFound
        }

        let python = try resolvePythonExecutable()
        await onLine("Python: \(python)")

        let process = Process()
        process.executableURL = URL(fileURLWithPath: python)

        var arguments = [
            helperURL.path,
            "--language", settings.language.rawValue,
            "--model", settings.modelName,
            "--buffer", String(settings.bufferSeconds)
        ]

        if settings.includeTimecodes {
            arguments.append("--timecodes")
        }

        if let outputFolder = settings.outputFolder {
            arguments.append(contentsOf: ["--output-dir", outputFolder.path])
        }

        for file in files {
            arguments.append(contentsOf: ["--file", file.url.path])
        }

        process.arguments = arguments
        process.environment = processEnvironment()

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        try process.run()

        let handle = pipe.fileHandleForReading
        Task.detached {
            while true {
                let data = handle.availableData
                if data.isEmpty { break }
                if let line = String(data: data, encoding: .utf8), !line.isEmpty {
                    await onLine(line.trimmingCharacters(in: .whitespacesAndNewlines))
                }
            }
        }

        process.waitUntilExit()
        if process.terminationStatus != 0 {
            throw TranscriptionRunnerError.pythonFailed(process.terminationStatus)
        }
    }

    private func resolvePythonExecutable() throws -> String {
        let candidates = pythonCandidates()
        var checked: [String] = []

        for candidate in candidates {
            let path = candidate.standardizedFileURL.path
            guard FileManager.default.isExecutableFile(atPath: path) else {
                checked.append("\(path) (not executable)")
                continue
            }
            if pythonSupportsDependencies(path) {
                return path
            }
            checked.append("\(path) (missing torch/openai-whisper)")
        }

        throw TranscriptionRunnerError.compatiblePythonNotFound(checked)
    }

    private func pythonCandidates() -> [URL] {
        var candidates: [URL] = []
        let bundleURL = Bundle.main.bundleURL
        let appProjectURL = bundleURL.deletingLastPathComponent().deletingLastPathComponent()
        let repositoryURL = appProjectURL.deletingLastPathComponent()

        candidates.append(appProjectURL.appendingPathComponent(".venv/bin/python3"))
        candidates.append(repositoryURL.appendingPathComponent("Transcription macOS/.venv/bin/python3"))
        candidates.append(URL(fileURLWithPath: "/opt/homebrew/bin/python3"))
        candidates.append(URL(fileURLWithPath: "/usr/local/bin/python3"))
        candidates.append(URL(fileURLWithPath: "/usr/bin/python3"))

        return candidates
    }

    private func pythonSupportsDependencies(_ pythonPath: String) -> Bool {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: pythonPath)
        process.arguments = ["-c", dependencyCheckCode]
        process.environment = processEnvironment()
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

    private func processEnvironment() -> [String: String] {
        var environment = ProcessInfo.processInfo.environment
        environment["PATH"] = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        environment["PYTORCH_ENABLE_MPS_FALLBACK"] = "1"
        return environment
    }

    private func helperScriptURL() -> URL? {
        if let resourceURL = Bundle.main.url(forResource: "transcribe_bulk", withExtension: "py") {
            return resourceURL
        }

        let developmentURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Resources/transcribe_bulk.py")
        return FileManager.default.fileExists(atPath: developmentURL.path) ? developmentURL : nil
    }
}
