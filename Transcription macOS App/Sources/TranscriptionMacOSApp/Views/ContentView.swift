import SwiftUI
import AppKit

struct ContentView: View {
    @State private var files: [SelectedMediaFile] = []
    @State private var language: TranscriptLanguage = .german
    @State private var modelName = "large"
    @State private var includeTimecodes = true
    @State private var bufferSeconds = 2
    @State private var useSourceFolder = true
    @State private var outputFolder: URL?
    @State private var logLines: [String] = ["Bereit."]
    @State private var isRunning = false

    private let runner = PythonTranscriptionRunner()
    private let models = ["tiny", "base", "small", "medium", "large"]

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Button("Dateien") { selectFiles() }
                Button("Leeren") { files.removeAll() }
                    .disabled(files.isEmpty || isRunning)
            }

            List(files) { file in
                VStack(alignment: .leading, spacing: 2) {
                    Text(file.name)
                        .lineLimit(1)
                    Text(file.folder)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            .listStyle(.sidebar)
        }
        .padding()
        .navigationSplitViewColumnWidth(min: 280, ideal: 320)
    }

    private var detail: some View {
        VStack(alignment: .leading, spacing: 16) {
            Form {
                Picker("Sprache", selection: $language) {
                    ForEach(TranscriptLanguage.allCases) { language in
                        Text(language.title).tag(language)
                    }
                }

                Picker("Whisper/Wispr Modell", selection: $modelName) {
                    ForEach(models, id: \.self) { model in
                        Text(model).tag(model)
                    }
                }

                Toggle("Timecodes schreiben", isOn: $includeTimecodes)

                Stepper("Buffer zwischen Dateien: \(bufferSeconds) Sekunden", value: $bufferSeconds, in: 0...60)

                Toggle("Markdown neben Quelldatei speichern", isOn: $useSourceFolder)

                HStack {
                    Text(outputFolder?.path ?? "Kein Zielordner ausgewaehlt")
                        .foregroundStyle(useSourceFolder ? .secondary : .primary)
                        .lineLimit(1)
                    Spacer()
                    Button("Zielordner") { selectOutputFolder() }
                        .disabled(useSourceFolder || isRunning)
                }
            }
            .formStyle(.grouped)

            HStack {
                Text("\(files.count) Datei(en) ausgewaehlt")
                    .foregroundStyle(.secondary)
                Spacer()
                Button(isRunning ? "Laeuft ..." : "Transkription starten") {
                    startTranscription()
                }
                .buttonStyle(.borderedProminent)
                .disabled(files.isEmpty || isRunning || (!useSourceFolder && outputFolder == nil))
            }

            Divider()

            Text("Protokoll")
                .font(.headline)

            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(Array(logLines.enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(.system(.caption, design: .monospaced))
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.vertical, 4)
            }
        }
        .padding()
    }

    private func selectFiles() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.audio, .movie, .mpeg4Movie, .quickTimeMovie]

        if panel.runModal() == .OK {
            let existing = Set(files.map(\.url))
            let additions = panel.urls.filter { !existing.contains($0) }.map(SelectedMediaFile.init(url:))
            files.append(contentsOf: additions)
        }
    }

    private func selectOutputFolder() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseFiles = false
        panel.canChooseDirectories = true

        if panel.runModal() == .OK {
            outputFolder = panel.url
        }
    }

    private func startTranscription() {
        let settings = TranscriptionSettings(
            language: language,
            modelName: modelName,
            includeTimecodes: includeTimecodes,
            bufferSeconds: bufferSeconds,
            outputFolder: useSourceFolder ? nil : outputFolder
        )

        isRunning = true
        appendLog("Starte Transkription.")

        Task {
            do {
                try await runner.run(files: files, settings: settings) { line in
                    appendLog(line)
                }
                appendLog("Fertig.")
            } catch {
                appendLog("FEHLER: \(error.localizedDescription)")
            }
            isRunning = false
        }
    }

    @MainActor
    private func appendLog(_ line: String) {
        guard !line.isEmpty else { return }
        logLines.append("[\(Self.timeFormatter.string(from: Date()))] \(line)")
    }

    private static let timeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"
        return formatter
    }()
}
