import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct ContentView: View {
    @EnvironmentObject private var controller: TranscriptionController
    @Environment(\.openWindow) private var openWindow
    @AppStorage("transcription.language") private var languageRaw = TranscriptLanguage.german.rawValue
    @AppStorage("transcription.model") private var modelRaw = WhisperModel.turbo.rawValue
    @AppStorage("transcription.timecodes") private var includeTimecodes = true
    @AppStorage("transcription.diarization") private var diarizationEnabled = true
    @AppStorage("transcription.speakerRange") private var speakerRangeRaw = SpeakerRange.automatic.rawValue
    @AppStorage("transcription.separation") private var separationRaw = SeparationPreset.normal.rawValue
    @AppStorage("transcription.outputVTT") private var outputVTT = false
    @AppStorage("transcription.outputTXT") private var outputTXT = false
    @AppStorage("transcription.outputCSV") private var outputCSV = false
    @AppStorage("transcription.outputSRT") private var outputSRT = false
    @AppStorage("transcription.outputMarkdown") private var outputMarkdown = true
    @AppStorage("transcription.useSourceFolder") private var useSourceFolder = true
    @State private var outputFolder: URL?
    @State private var dropTargeted = false
    @State private var resultSearch = ""
    @AppStorage("transcription.showArchivedResults") private var showArchivedResults = false
    @State private var pendingResultDeletion: TranscriptionResult?
    @State private var showDiagnostics = false
    @State private var showGlossaryManager = false
    @State private var showMediaBrowser = false

    init() {
        _outputFolder = State(initialValue: BookmarkStore.loadOutputFolder())
    }

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            detail
        }
        .alert(item: $pendingResultDeletion) { result in
            Alert(
                title: Text("Bearbeitungsstand in den Papierkorb bewegen?"),
                message: Text("Der interne Bearbeitungsstand für „\(result.sourceURL.lastPathComponent)“ wird entfernt. Quelldatei und bereits exportierte Dateien bleiben erhalten."),
                primaryButton: .destructive(Text("In den Papierkorb")) {
                    controller.moveInternalRecordToTrash(result)
                },
                secondaryButton: .cancel()
            )
        }
        .sheet(isPresented: $showDiagnostics) {
            DiagnosticsView(runner: controller.runner)
        }
        .sheet(isPresented: $showGlossaryManager) {
            GlossaryManagerView()
        }
        .sheet(isPresented: $showMediaBrowser) {
            MediaBrowserView(settingsProvider: { settings }) { files, autostart, snapshot in
                controller.addDownloadedMediaFiles(files)
                if autostart { controller.start(files: files, settings: snapshot) }
            }
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Button("Dateien hinzufügen") { selectFiles() }
                    Button("Medien laden") { showMediaBrowser = true }
                        .disabled(controller.isRunning)
                }
                HStack(spacing: 8) {
                    Button("VTT importieren") { importVTT() }
                    Spacer()
                    Button("Leeren") { controller.clearFiles() }
                        .disabled(controller.files.isEmpty || controller.isRunning)
                }
            }
            if controller.files.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "waveform.badge.plus")
                        .font(.system(size: 32))
                        .foregroundStyle(dropTargeted ? Color.accentColor : Color.secondary)
                    Text(dropTargeted ? "Dateien hier ablegen" : "Keine Dateien ausgewählt")
                        .font(.headline)
                    Text("Audio, Video oder heruntergeladene Medien hinzufügen.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List {
                    ForEach(controller.files) { file in
                        HStack {
                            VStack(alignment: .leading, spacing: 3) {
                                Text(file.name).lineLimit(1)
                                Text(file.folder).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                            }
                            Spacer()
                            Button { controller.remove(file) } label: {
                                Image(systemName: "xmark.circle.fill")
                            }
                            .buttonStyle(.plain)
                            .foregroundStyle(.secondary)
                            .disabled(controller.isRunning)
                        }
                        .padding(.vertical, 3)
                    }
                    .onMove { controller.files.move(fromOffsets: $0, toOffset: $1) }
                }
                .listStyle(.sidebar)
            }
        }
        .padding()
        .navigationSplitViewColumnWidth(min: 330, ideal: 380)
        .onDrop(of: [UTType.fileURL.identifier], isTargeted: $dropTargeted, perform: receiveDrop)
    }

    private var detail: some View {
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text("Transcription macOS").font(.title2.bold())
                    Spacer()
                    Button("Wörterbuch") { showGlossaryManager = true }
                    Button("System prüfen") { showDiagnostics = true }
                }
                Text("Lokale Transkription mit Sprechererkennung und vollständiger Nachbearbeitung.")
                    .foregroundStyle(.secondary)
            }
            settingsPanel
            runBar
            if controller.results.isEmpty { logPanel } else { resultAndLogPanel }
        }
        .padding(28)
    }

    private var settingsPanel: some View {
        Grid(alignment: .leading, horizontalSpacing: 20, verticalSpacing: 12) {
            GridRow {
                Text("Sprache")
                HStack {
                    Picker("Sprache", selection: $languageRaw) {
                        ForEach(TranscriptLanguage.allCases) { Text($0.title).tag($0.rawValue) }
                    }.labelsHidden()
                    if languageRaw == TranscriptLanguage.mixed.rawValue {
                        Text("Whisper erkennt Deutsch und Englisch automatisch innerhalb der Aufnahme.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            GridRow {
                Text("Whisper-Modell")
                Picker("Whisper-Modell", selection: $modelRaw) {
                    ForEach(WhisperModel.allCases) { Text($0.title).tag($0.rawValue) }
                }.labelsHidden()
            }
            GridRow {
                Text("Sprecher")
                HStack(spacing: 12) {
                    Toggle("Sprecher erkennen", isOn: $diarizationEnabled)
                    Picker("Anzahl", selection: $speakerRangeRaw) {
                        ForEach(SpeakerRange.allCases) { Text($0.title).tag($0.rawValue) }
                    }
                    .labelsHidden()
                    .disabled(!diarizationEnabled)
                }
            }
            GridRow {
                Text("Trennung")
                HStack {
                    Picker("Trennung", selection: $separationRaw) {
                        ForEach(SeparationPreset.allCases) { Text($0.title).tag($0.rawValue) }
                    }
                    .labelsHidden()
                    .disabled(!diarizationEnabled)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            GridRow {
                Text("Ausgaben")
                HStack(spacing: 14) {
                    Toggle("Markdown", isOn: $outputMarkdown)
                    Toggle("VTT", isOn: $outputVTT)
                    Toggle("SRT", isOn: $outputSRT)
                    Toggle("TXT", isOn: $outputTXT)
                    Toggle("CSV", isOn: $outputCSV)
                    Toggle("Zeitcodes", isOn: $includeTimecodes)
                }
            }
            GridRow {
                Text("Speicherort")
                HStack {
                    Toggle("Neben Quelldatei", isOn: $useSourceFolder)
                    if !useSourceFolder {
                        Text(outputFolder?.path ?? "Kein Ziel gewählt").foregroundStyle(.secondary).lineLimit(1)
                        Spacer()
                        Button("Ordner wählen") { selectOutputFolder() }
                    }
                }
            }
        }
        .disabled(controller.isRunning)
        .padding(18)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
    }

    private var runBar: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(selectionText).font(.headline)
                Text(controller.progressLine).font(.caption).foregroundStyle(controller.isRunning ? .primary : .secondary).lineLimit(1)
                if !controller.estimateLine.isEmpty {
                    Text(controller.estimateLine).font(.caption2).foregroundStyle(.secondary)
                }
                if controller.isRunning { ProgressView(value: controller.progressValue).frame(maxWidth: 320) }
            }
            Spacer()
            if controller.isRunning {
                Button("Abbrechen", role: .destructive) { controller.cancel() }
            }
            Button(controller.isRunning ? "Läuft …" : "Transkription starten") {
                controller.start(settings: settings)
            }
            .buttonStyle(.borderedProminent)
            .disabled(!canRun || controller.isRunning)
        }
    }

    private var resultAndLogPanel: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Ergebnisse").font(.headline)
                Spacer()
                TextField("Ergebnisse durchsuchen", text: $resultSearch)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 230)
                Toggle("Archiv anzeigen", isOn: $showArchivedResults)
                    .toggleStyle(.checkbox)
            }
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(visibleResults) { result in
                        HStack(spacing: 10) {
                            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(result.sourceURL.lastPathComponent).lineLimit(1)
                                Text("\(result.segmentCount) Segmente · \(result.speakerCount) Sprecher · \(result.outputs.keys.sorted().joined(separator: ", "))")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button("Bearbeiten") { openEditor(result) }
                            Button("Im Finder") { controller.reveal(result: result) }
                            Menu {
                                Button(controller.isArchived(result) ? "Aus Archiv holen" : "Archivieren") {
                                    controller.toggleArchive(result)
                                }
                                Divider()
                                Button("Bearbeitungsstand löschen …", role: .destructive) {
                                    pendingResultDeletion = result
                                }
                            } label: {
                                Image(systemName: "ellipsis.circle")
                            }
                            .menuStyle(.borderlessButton)
                        }
                        .padding(10)
                        .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(.separator, lineWidth: 1))
                    }
                }
            }
            .frame(maxHeight: 280)
            if visibleResults.isEmpty {
                Text(resultSearch.isEmpty ? "Keine Ergebnisse in dieser Ansicht." : "Keine passenden Ergebnisse gefunden.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            DisclosureGroup("Aktivität") { logContent.frame(minHeight: 120) }
        }
    }

    private var logPanel: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Aktivität").font(.headline)
                Spacer()
                Button("Protokoll leeren") { controller.clearLog() }.disabled(controller.isRunning)
            }
            logContent
        }
    }

    private var logContent: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 8) {
                    ForEach(controller.logEntries) { entry in
                        HStack(alignment: .firstTextBaseline, spacing: 10) {
                            Text(Self.timeFormatter.string(from: entry.timestamp))
                                .font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).frame(width: 62)
                            Circle().fill(color(for: entry.kind)).frame(width: 7, height: 7)
                            Text(entry.message).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                        }
                        .id(entry.id)
                    }
                }.padding(12)
            }
            .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(.separator, lineWidth: 1))
            .onChange(of: controller.logEntries) { entries in
                if let last = entries.last { proxy.scrollTo(last.id, anchor: .bottom) }
            }
        }
    }

    private var settings: TranscriptionSettings {
        TranscriptionSettings(
            language: TranscriptLanguage(rawValue: languageRaw) ?? .german,
            model: WhisperModel(rawValue: modelRaw) ?? .turbo,
            includeTimecodes: includeTimecodes,
            diarizationEnabled: diarizationEnabled,
            speakerRange: SpeakerRange(rawValue: speakerRangeRaw) ?? .automatic,
            separation: SeparationPreset(rawValue: separationRaw) ?? .normal,
            outputFormats: selectedFormats,
            outputFolder: useSourceFolder ? nil : outputFolder
        )
    }

    private var selectedFormats: Set<OutputFormat> {
        var formats: Set<OutputFormat> = []
        if outputMarkdown { formats.insert(.markdown) }
        if outputVTT { formats.insert(.vtt) }
        if outputSRT { formats.insert(.srt) }
        if outputTXT { formats.insert(.txt) }
        if outputCSV { formats.insert(.csv) }
        return formats
    }

    private var visibleResults: [TranscriptionResult] {
        let query = resultSearch.trimmingCharacters(in: .whitespacesAndNewlines)
        return controller.results.filter { result in
            (showArchivedResults || !controller.isArchived(result))
                && (query.isEmpty
                    || result.sourceURL.lastPathComponent.localizedCaseInsensitiveContains(query)
                    || result.sourceURL.deletingLastPathComponent().path.localizedCaseInsensitiveContains(query))
        }
    }

    private var canRun: Bool {
        !controller.files.isEmpty && !selectedFormats.isEmpty && (useSourceFolder || outputFolder != nil)
    }

    private var selectionText: String {
        controller.files.isEmpty ? "Keine Dateien ausgewählt" : "\(controller.files.count) Datei\(controller.files.count == 1 ? "" : "en") ausgewählt"
    }

    private func selectFiles() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = MediaFileSupport.openPanelContentTypes
        if panel.runModal() == .OK { controller.addFiles(panel.urls) }
    }

    private func selectOutputFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        if panel.runModal() == .OK {
            outputFolder = panel.url
            BookmarkStore.saveOutputFolder(panel.url)
        }
    }

    private func importVTT() {
        let vttPanel = NSOpenPanel()
        vttPanel.allowedContentTypes = [UTType(filenameExtension: "vtt") ?? .plainText]
        guard vttPanel.runModal() == .OK, let vttURL = vttPanel.url else { return }
        let audioPanel = NSOpenPanel()
        audioPanel.message = "Optional: zugehörige Audio- oder Videodatei für die Wiedergabe wählen. Abbrechen importiert ohne Audio."
        audioPanel.allowedContentTypes = MediaFileSupport.openPanelContentTypes
        _ = audioPanel.runModal()
        do {
            let result = try controller.importVTT(vttURL: vttURL, audioURL: audioPanel.url)
            openEditor(result)
        } catch { controller.appendLog(error.localizedDescription, kind: .error) }
    }

    private func openEditor(_ result: TranscriptionResult) {
        openWindow(value: result.documentURL.path)
    }

    private func receiveDrop(_ providers: [NSItemProvider]) -> Bool {
        var accepted = false
        for provider in providers where provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
            accepted = true
            provider.loadDataRepresentation(forTypeIdentifier: UTType.fileURL.identifier) { data, _ in
                guard let data, let url = URL(dataRepresentation: data, relativeTo: nil) else { return }
                Task { @MainActor in controller.addFiles([url]) }
            }
        }
        return accepted
    }

    private func color(for kind: LogEntry.Kind) -> Color {
        switch kind { case .info: return .secondary; case .success: return .green; case .error: return .red }
    }

    private static let timeFormatter: DateFormatter = {
        let formatter = DateFormatter(); formatter.dateFormat = "HH:mm:ss"; return formatter
    }()
}
