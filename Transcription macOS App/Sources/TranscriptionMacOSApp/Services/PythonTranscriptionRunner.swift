import Foundation

enum TranscriptionRunnerError: LocalizedError {
    case helperNotFound
    case pythonFailed(Int32)

    var errorDescription: String? {
        switch self {
        case .helperNotFound:
            return "Python helper transcribe_bulk.py was not found in the app resources."
        case .pythonFailed(let code):
            return "Python transcription process exited with code \(code)."
        }
    }
}

final class PythonTranscriptionRunner {
    func run(files: [SelectedMediaFile], settings: TranscriptionSettings, onLine: @escaping @MainActor (String) -> Void) async throws {
        guard let helperURL = helperScriptURL() else {
            throw TranscriptionRunnerError.helperNotFound
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")

        var arguments = [
            "python3",
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
