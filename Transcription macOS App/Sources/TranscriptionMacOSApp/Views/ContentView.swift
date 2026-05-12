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
    @State private var logEntries: [LogEntry] = [
        LogEntry(timestamp: Date(), message: "Ready.", kind: .info)
    ]
    @State private var progressLine = "Idle"
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
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                Button("Add Files") { selectFiles() }
                Button("Clear") { files.removeAll() }
                    .disabled(files.isEmpty || isRunning)
            }

            if files.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "waveform")
                        .font(.system(size: 32))
                        .foregroundStyle(.secondary)
                    Text("No files selected")
                        .font(.headline)
                    Text("Add one or more audio or video files.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(files) { file in
                    VStack(alignment: .leading, spacing: 3) {
                        Text(file.name)
                            .font(.body)
                            .lineLimit(1)
                        Text(file.folder)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    .padding(.vertical, 3)
                }
                .listStyle(.sidebar)
            }
        }
        .padding()
        .navigationSplitViewColumnWidth(min: 300, ideal: 360)
    }

    private var detail: some View {
        VStack(alignment: .leading, spacing: 18) {
            header
            settingsPanel
            runBar
            logPanel
        }
        .padding(28)
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Transcription macOS")
                .font(.title2.bold())
            Text("Bulk transcribe local media files with Whisper.")
                .foregroundStyle(.secondary)
        }
    }

    private var settingsPanel: some View {
        Grid(alignment: .leading, horizontalSpacing: 20, verticalSpacing: 14) {
            GridRow {
                Text("Language")
                Picker("Language", selection: $language) {
                    ForEach(TranscriptLanguage.allCases) { language in
                        Text(language.title).tag(language)
                    }
                }
                .labelsHidden()
            }

            GridRow {
                Text("Whisper model")
                Picker("Whisper model", selection: $modelName) {
                    ForEach(models, id: \.self) { model in
                        Text(model).tag(model)
                    }
                }
                .labelsHidden()
            }

            GridRow {
                Text("Write timecodes")
                Toggle("Write timecodes", isOn: $includeTimecodes)
                    .labelsHidden()
            }

            GridRow {
                Text("File buffer")
                Stepper("\(bufferSeconds) \(bufferSeconds == 1 ? "second" : "seconds")", value: $bufferSeconds, in: 0...60)
            }

            GridRow {
                Text("Output")
                Toggle("Save Markdown next to source files", isOn: $useSourceFolder)
            }

            GridRow {
                Text("Destination")
                HStack {
                    Text(outputFolder?.path ?? "No destination folder selected")
                        .foregroundStyle(useSourceFolder ? .secondary : .primary)
                        .lineLimit(1)
                    Spacer()
                    Button("Choose Folder") { selectOutputFolder() }
                        .disabled(useSourceFolder || isRunning)
                }
            }
        }
        .padding(18)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
    }

    private var runBar: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 3) {
                Text(fileSelectionText)
                    .font(.headline)
                Text(progressLine)
                    .font(.caption)
                    .foregroundStyle(isRunning ? .primary : .secondary)
                    .lineLimit(1)
            }

            Spacer()

            if isRunning {
                ProgressView()
                    .controlSize(.small)
            }

            Button(isRunning ? "Running..." : "Start Transcription") {
                startTranscription()
            }
            .buttonStyle(.borderedProminent)
            .disabled(files.isEmpty || isRunning || (!useSourceFolder && outputFolder == nil))
        }
    }

    private var logPanel: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Activity")
                    .font(.headline)
                Spacer()
                Button("Clear Log") {
                    logEntries = [LogEntry(timestamp: Date(), message: "Ready.", kind: .info)]
                    progressLine = isRunning ? progressLine : "Idle"
                }
                .disabled(isRunning)
            }

            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        ForEach(logEntries) { entry in
                            logRow(entry)
                                .id(entry.id)
                        }
                    }
                    .padding(12)
                }
                .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(.separator, lineWidth: 1)
                )
                .onChange(of: logEntries) { entries in
                    guard let last = entries.last else { return }
                    withAnimation(.easeOut(duration: 0.18)) {
                        proxy.scrollTo(last.id, anchor: .bottom)
                    }
                }
            }
        }
    }

    private func logRow(_ entry: LogEntry) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(Self.timeFormatter.string(from: entry.timestamp))
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(width: 62, alignment: .leading)

            Circle()
                .fill(color(for: entry.kind))
                .frame(width: 7, height: 7)

            Text(entry.message)
                .font(.system(.caption, design: .monospaced))
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var fileSelectionText: String {
        switch files.count {
        case 0: return "No files selected"
        case 1: return "1 file selected"
        default: return "\(files.count) files selected"
        }
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
            appendLog("Added \(additions.count) \(additions.count == 1 ? "file" : "files").", kind: .info)
        }
    }

    private func selectOutputFolder() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = false
        panel.canChooseFiles = false
        panel.canChooseDirectories = true

        if panel.runModal() == .OK, let selectedURL = panel.url {
            outputFolder = selectedURL
            appendLog("Output folder: \(selectedURL.path)", kind: .info)
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
        progressLine = "Preparing transcription..."
        appendLog("Started transcription.", kind: .info)

        Task {
            do {
                try await runner.run(files: files, settings: settings) { line in
                    handleRunnerLine(line)
                }
                progressLine = "Completed"
                appendLog("Finished.", kind: .success)
            } catch {
                progressLine = "Failed"
                appendLog(error.localizedDescription, kind: .error)
            }
            isRunning = false
        }
    }

    @MainActor
    private func handleRunnerLine(_ payload: String) {
        for line in payload.components(separatedBy: .newlines) {
            let cleaned = line.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !cleaned.isEmpty else { continue }

            if isProgressLine(cleaned) {
                progressLine = compactProgress(cleaned)
            } else {
                appendLog(cleaned, kind: kind(for: cleaned))
            }
        }
    }

    @MainActor
    private func appendLog(_ message: String, kind: LogEntry.Kind) {
        guard !message.isEmpty else { return }
        logEntries.append(LogEntry(timestamp: Date(), message: message, kind: kind))
    }

    private func isProgressLine(_ line: String) -> Bool {
        line.contains("%|") || line.contains("frames/s]") || line.contains("it/s]")
    }

    private func compactProgress(_ line: String) -> String {
        if let percentRange = line.range(of: #"(?<!\d)(\d{1,3})%"#, options: .regularExpression) {
            let percent = String(line[percentRange])
            if let etaRange = line.range(of: #"<[^,\]]+"#, options: .regularExpression) {
                return "Transcribing... \(percent), ETA \(line[etaRange].dropFirst())"
            }
            return "Transcribing... \(percent)"
        }
        return "Transcribing..."
    }

    private func kind(for line: String) -> LogEntry.Kind {
        let lowercased = line.lowercased()
        if lowercased.contains("error") || lowercased.contains("failed") || lowercased.contains("traceback") {
            return .error
        }
        if lowercased.contains("saved:") || lowercased.contains("complete") || lowercased.contains("loaded") {
            return .success
        }
        return .info
    }

    private func color(for kind: LogEntry.Kind) -> Color {
        switch kind {
        case .info: return .secondary
        case .success: return .green
        case .error: return .red
        }
    }

    private static let timeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss"
        return formatter
    }()
}
