import AVFoundation
import AppKit
import SwiftUI

struct TranscriptEditorView: View {
    private enum Section: String, CaseIterable, Identifiable {
        case transcript = "Transkript"
        case speakers = "Sprecher"
        case glossary = "Begriffe"
        var id: String { rawValue }
    }

    @Environment(\.dismiss) private var dismiss
    @Environment(\.undoManager) private var undoManager
    @State private var document: TranscriptDocument
    @State private var section: Section = .transcript
    @State private var status = ""
    @State private var isSaving = false
    @State private var glossary: [GlossaryCandidate] = []
    @State private var replacements: [String: String] = [:]
    @State private var isAnalyzingGlossary = false
    @State private var bulkSpeaker = ""
    @State private var exportFormats: Set<OutputFormat>
    @State private var pendingSpeakerMerge: SpeakerMerge?
    @State private var autosaveTask: Task<Void, Never>?
    @State private var transcriptSearch = ""
    @StateObject private var playback: AudioPlaybackController
    @StateObject private var undoCoordinator = TranscriptUndoCoordinator()

    let result: TranscriptionResult
    let runner: PythonTranscriptionRunner
    let onSaved: (TranscriptDocument) -> Void

    init(
        initialDocument: TranscriptDocument,
        result: TranscriptionResult,
        runner: PythonTranscriptionRunner,
        onSaved: @escaping (TranscriptDocument) -> Void
    ) {
        _document = State(initialValue: initialDocument)
        _exportFormats = State(initialValue: Set(initialDocument.outputs.keys.compactMap(OutputFormat.init(rawValue:))) .isEmpty ? [.vtt] : Set(initialDocument.outputs.keys.compactMap(OutputFormat.init(rawValue:))))
        _playback = StateObject(wrappedValue: AudioPlaybackController(url: result.sourceURL))
        self.result = result
        self.runner = runner
        self.onSaved = onSaved
    }

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            Picker("Bereich", selection: $section) {
                ForEach(Section.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .frame(maxWidth: 420)
            .padding()
            Group {
                switch section {
                case .transcript: transcriptEditor
                case .speakers: speakerEditor
                case .glossary: glossaryEditor
                }
            }
            playbackBar
        }
        .onChange(of: document.segments) { _ in scheduleAutosave() }
        .onChange(of: document.speakerNames) { _ in scheduleAutosave() }
        .onChange(of: document.speakerRegions) { _ in scheduleAutosave() }
        .onAppear {
            undoCoordinator.apply = { restored in document = restored }
            if document.language == TranscriptLanguage.mixed.rawValue {
                SegmentLanguageSupport.annotate(&document)
            }
        }
        .alert(item: $pendingSpeakerMerge) { merge in
            Alert(
                title: Text("Sprecher zusammenführen?"),
                message: Text("Alle Abschnitte von „\(displayName(merge.source))“ werden „\(displayName(merge.target))“ zugeordnet. Die Zusammenführung wird automatisch gespeichert und kann mit „Rückgängig“ zurückgenommen werden."),
                primaryButton: .destructive(Text("Zusammenführen")) { mergeSpeakers(merge) },
                secondaryButton: .cancel()
            )
        }
        .onDisappear {
            autosaveTask?.cancel()
            undoCoordinator.apply = nil
            persistDraft(showStatus: false)
            playback.stop()
        }
    }

    private var toolbar: some View {
        HStack(spacing: 12) {
            Button("Schließen") { dismiss() }
            Button { undoManager?.undo() } label: { Image(systemName: "arrow.uturn.backward") }
                .help("Rückgängig (⌘Z)")
                .disabled(undoManager?.canUndo != true)
                .keyboardShortcut("z", modifiers: .command)
            Button { undoManager?.redo() } label: { Image(systemName: "arrow.uturn.forward") }
                .help("Wiederholen (⇧⌘Z)")
                .disabled(undoManager?.canRedo != true)
                .keyboardShortcut("z", modifiers: [.command, .shift])
            VStack(alignment: .leading, spacing: 2) {
                Text(document.sourceFile).font(.headline).lineLimit(1)
                Text(status.isEmpty ? "\(document.segments.count) Segmente" : status)
                    .font(.caption).foregroundStyle(status.hasPrefix("Fehler") ? .red : .secondary)
            }
            Spacer()
            Menu("Formate") {
                ForEach(OutputFormat.allCases) { format in
                    Toggle(format.title, isOn: formatBinding(format))
                }
            }
            .fixedSize()
            Button("Exportieren …") { exportDocument() }.disabled(isSaving || isAnalyzingGlossary || exportFormats.isEmpty)
            Button(isSaving ? "Speichert …" : "Speichern") { saveDocument() }
                .buttonStyle(.borderedProminent)
                .disabled(isSaving || isAnalyzingGlossary)
        }
        .padding(14)
    }

    private var transcriptEditor: some View {
        VStack(spacing: 10) {
            HStack {
                Text("Sprecher für alle Segmente")
                Picker("Sprecher", selection: $bulkSpeaker) {
                    Text("Auswählen …").tag("")
                    ForEach(speakerLabels, id: \.self) { label in
                        Text(document.speakerNames[label] ?? label).tag(label)
                    }
                }.labelsHidden().frame(maxWidth: 220)
                Button("Allen zuweisen") {
                    guard !bulkSpeaker.isEmpty else { return }
                    performUndoable("Sprecher allen zuweisen") { draft in
                        for index in draft.segments.indices { draft.segments[index].speaker = bulkSpeaker }
                    }
                }.disabled(bulkSpeaker.isEmpty)
                Button("Nur leeren zuweisen") {
                    guard !bulkSpeaker.isEmpty else { return }
                    performUndoable("Leere Sprecher zuweisen") { draft in
                        for index in draft.segments.indices where draft.segments[index].speaker == nil {
                            draft.segments[index].speaker = bulkSpeaker
                        }
                    }
                }.disabled(bulkSpeaker.isEmpty)
                Spacer()
                TextField("Transkript durchsuchen", text: $transcriptSearch)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 240)
            }
            .padding(.horizontal, 16)

            List {
                ForEach($document.segments) { $segment in
                    if matchesSearch(segment) {
                        HStack(alignment: .top, spacing: 10) {
                            VStack(alignment: .leading, spacing: 6) {
                                Text("\(clock(segment.start)) – \(clock(segment.end))")
                                    .font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                                if let language = SegmentLanguageSupport.title(for: segment.language) {
                                    Text(language)
                                        .font(.caption2.bold())
                                        .foregroundStyle(.secondary)
                                }
                                Button("Anhören") { playback.play(start: segment.start, end: segment.end) }
                            }
                            .frame(width: 110, alignment: .leading)
                            Picker("Sprecher", selection: $segment.speaker) {
                                Text("Ohne Sprecher").tag(String?.none)
                                ForEach(speakerLabels, id: \.self) { label in
                                    Text(document.speakerNames[label] ?? label).tag(String?.some(label))
                                }
                            }
                            .labelsHidden()
                            .frame(width: 150)
                            TextEditor(text: $segment.text)
                                .font(.body)
                                .frame(minHeight: 54)
                            Menu {
                                Button("Segment teilen") {
                                    performUndoable("Segment teilen") { draft in
                                        TranscriptEditingSupport.split(segmentID: segment.id, in: &draft)
                                    }
                                }
                                .disabled(segment.text.split(separator: " ").count < 2)
                                Button("Mit nächstem Segment verbinden") {
                                    performUndoable("Segmente verbinden") { draft in
                                        TranscriptEditingSupport.mergeWithNext(segmentID: segment.id, in: &draft)
                                    }
                                }
                                .disabled(document.segments.last?.id == segment.id)
                                Divider()
                                Button("Segment löschen", role: .destructive) {
                                    performUndoable("Segment löschen") { draft in
                                        draft.segments.removeAll { $0.id == segment.id }
                                    }
                                }
                            } label: {
                                Image(systemName: "ellipsis.circle")
                            }
                            .menuStyle(.borderlessButton)
                            .menuIndicator(.hidden)
                            .frame(width: 24)
                        }.padding(.vertical, 5)
                    }
                }
            }
        }
    }

    private var speakerEditor: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                Text("Benennen oder prüfen Sie die erkannten Stimmen. Eine Hörprobe verwendet den längsten verfügbaren Abschnitt; falsch getrennte Stimmen können zusammengeführt werden.")
                    .foregroundStyle(.secondary)
                if speakerLabels.isEmpty {
                    emptyState(
                        document.diarization
                            ? "Keine Sprecher erkannt"
                            : "Sprechererkennung war für diese Transkription ausgeschaltet",
                        symbol: "person.2.slash"
                    )
                }
                ForEach(speakerLabels, id: \.self) { label in
                    HStack(spacing: 12) {
                        Image(systemName: "person.wave.2")
                        TextField("Sprechername", text: speakerNameBinding(label)).frame(maxWidth: 320)
                        Button("Hörprobe") {
                            if let window = sampleWindow(for: label) { playback.play(start: window.0, end: window.1, loop: true) }
                        }
                        Menu("Zusammenführen …") {
                            ForEach(speakerLabels.filter { $0 != label }, id: \.self) { target in
                                Button("Mit \(displayName(target))") {
                                    pendingSpeakerMerge = SpeakerMerge(source: label, target: target)
                                }
                            }
                        }
                        .disabled(speakerLabels.count < 2)
                        Spacer()
                        Text(durationForSpeaker(label), format: .number.precision(.fractionLength(1)))
                        Text("Sek.").foregroundStyle(.secondary)
                    }
                    .padding(12)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
                }
            }.padding(20)
        }
    }

    private var glossaryEditor: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Eigennamen und Fachbegriffe vor dem Export prüfen.").foregroundStyle(.secondary)
                Spacer()
                if isAnalyzingGlossary {
                    ProgressView().controlSize(.small)
                }
                Button(glossary.isEmpty ? "Begriffe ermitteln" : "Neu ermitteln") { loadGlossary() }
                    .disabled(isAnalyzingGlossary || isSaving)
                Button("Ersetzungen anwenden") { applyGlossary() }
                    .buttonStyle(.borderedProminent)
                    .disabled(isAnalyzingGlossary || GlossarySupport.activeReplacementCount(replacements) == 0)
            }
            if glossary.isEmpty {
                emptyState("Noch keine Begriffe geprüft", symbol: "text.magnifyingglass")
            } else {
                List(glossary) { candidate in
                    HStack {
                        VStack(alignment: .leading) {
                            Text(candidate.term)
                            Text("\(candidate.kind) · \(candidate.count)×").font(.caption).foregroundStyle(.secondary)
                        }.frame(width: 220, alignment: .leading)
                        Image(systemName: "arrow.right").foregroundStyle(.secondary)
                        TextField("Neue Schreibweise", text: replacementBinding(candidate.term))
                    }.padding(.vertical, 4)
                }
            }
        }.padding(20)
    }

    private var playbackBar: some View {
        HStack(spacing: 12) {
            Text(clock(playback.currentTime)).font(.system(.caption, design: .monospaced)).frame(width: 60)
            Button("« 5 s") { playback.seek(by: -5) }
            Button(playback.isPlaying ? "Pause" : "Wiedergabe") { playback.toggle() }
                .buttonStyle(.borderedProminent)
            Button("5 s »") { playback.seek(by: 5) }
            Toggle("Schleife", isOn: $playback.loop)
            Button("\(playback.rate, specifier: "%.2g")×") { playback.cycleRate() }
            Spacer()
        }
        .padding(12)
        .background(.bar)
    }

    private var speakerLabels: [String] { SpeakerEditingSupport.labels(in: document) }

    private func emptyState(_ title: String, symbol: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: symbol).font(.system(size: 30)).foregroundStyle(.secondary)
            Text(title).font(.headline)
        }
        .frame(maxWidth: .infinity, minHeight: 180)
    }

    private func saveDocument() {
        autosaveTask?.cancel()
        autosaveTask = nil
        isSaving = true
        status = "Speichert …"
        Task {
            do {
                let outputs: [String: String]
                if document.outputs.isEmpty {
                    guard let folder = chooseExportFolder() else { isSaving = false; status = ""; return }
                    outputs = try await runner.export(document: document, to: folder, formats: exportFormats)
                    document.outputs = outputs
                } else {
                    outputs = try await runner.save(document: document, at: result.documentURL)
                }
                document.outputs = outputs
                _ = try runner.store(document: document)
                onSaved(document)
                status = "Gespeichert"
            } catch { status = "Fehler: \(error.localizedDescription)" }
            isSaving = false
        }
    }

    private func exportDocument() {
        guard let folder = chooseExportFolder() else { return }
        autosaveTask?.cancel()
        autosaveTask = nil
        isSaving = true
        status = "Exportiert …"
        Task {
            do {
                let outputs = try await runner.export(document: document, to: folder, formats: exportFormats)
                document.outputs.merge(outputs) { _, new in new }
                _ = try runner.store(document: document)
                onSaved(document)
                status = "Export abgeschlossen"
            } catch { status = "Fehler: \(error.localizedDescription)" }
            isSaving = false
        }
    }

    private func chooseExportFolder() -> URL? {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.prompt = "Exportieren"
        return panel.runModal() == .OK ? panel.url : nil
    }

    private func loadGlossary() {
        guard !isAnalyzingGlossary else { return }
        isAnalyzingGlossary = true
        status = "Analysiere Begriffe …"
        Task {
            defer { isAnalyzingGlossary = false }
            do {
                let candidates = try await runner.extractGlossary(document: document)
                let merged = GlossarySupport.merged(candidates)
                replacements = GlossarySupport.replacementMap(for: merged, preserving: replacements)
                glossary = merged
                status = merged.isEmpty ? "Keine Begriffe gefunden" : "\(merged.count) Begriffe gefunden"
            } catch { status = "Fehler: \(error.localizedDescription)" }
        }
    }

    private func applyGlossary() {
        let count = GlossarySupport.activeReplacementCount(replacements)
        performUndoable("Begriffsersetzungen anwenden") { draft in
            for index in draft.segments.indices {
                draft.segments[index].text = GlossarySupport.applying(replacements, to: draft.segments[index].text)
            }
        }
        GlossaryStore.add(replacements: replacements)
        status = "\(count) Ersetzungen angewendet"
    }

    private func sampleWindow(for label: String) -> (Double, Double)? {
        let regions = document.speakerRegions.filter { $0.speaker == label }
        if let longest = regions.max(by: { $0.end - $0.start < $1.end - $1.start }) {
            let duration = min(10, longest.end - longest.start)
            let start = max(longest.start, (longest.start + longest.end - duration) / 2)
            return (start, start + duration)
        }
        if let longest = document.segments.filter({ $0.speaker == label }).max(by: { $0.end - $0.start < $1.end - $1.start }) {
            return (longest.start, min(longest.end, longest.start + 10))
        }
        return nil
    }

    private func durationForSpeaker(_ label: String) -> Double {
        let regions = document.speakerRegions.filter { $0.speaker == label }
        if !regions.isEmpty { return regions.reduce(0) { $0 + $1.end - $1.start } }
        return document.segments.filter { $0.speaker == label }.reduce(0) { $0 + $1.end - $1.start }
    }

    private func speakerNameBinding(_ label: String) -> Binding<String> {
        Binding(get: { document.speakerNames[label] ?? label }, set: { document.speakerNames[label] = String($0.prefix(60)) })
    }

    private func mergeSpeakers(_ merge: SpeakerMerge) {
        let sourceName = displayName(merge.source)
        let targetName = displayName(merge.target)
        performUndoable("Sprecher zusammenführen") { draft in
            SpeakerEditingSupport.merge(merge.source, into: merge.target, in: &draft)
        }
        if bulkSpeaker == merge.source { bulkSpeaker = merge.target }
        status = "„\(sourceName)“ wurde mit „\(targetName)“ zusammengeführt"
        scheduleAutosave()
    }

    private func displayName(_ label: String) -> String {
        document.speakerNames[label] ?? label
    }

    private func matchesSearch(_ segment: TranscriptSegment) -> Bool {
        let query = transcriptSearch.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return true }
        return segment.text.localizedCaseInsensitiveContains(query)
            || displayName(segment.speaker ?? "").localizedCaseInsensitiveContains(query)
    }

    private func performUndoable(_ name: String, change: (inout TranscriptDocument) -> Void) {
        let previous = document
        change(&document)
        undoCoordinator.register(previous: previous, current: document, name: name, with: undoManager)
    }

    private func scheduleAutosave() {
        autosaveTask?.cancel()
        autosaveTask = Task {
            try? await Task.sleep(nanoseconds: 750_000_000)
            guard !Task.isCancelled else { return }
            persistDraft(showStatus: true)
        }
    }

    private func persistDraft(showStatus: Bool) {
        do {
            _ = try runner.store(document: document)
            onSaved(document)
            if showStatus && !isSaving && !isAnalyzingGlossary { status = "Entwurf automatisch gespeichert" }
        } catch {
            if showStatus { status = "Fehler beim automatischen Speichern: \(error.localizedDescription)" }
        }
    }

    private func replacementBinding(_ term: String) -> Binding<String> {
        Binding(get: { replacements[term] ?? "" }, set: { replacements[term] = $0 })
    }

    private func formatBinding(_ format: OutputFormat) -> Binding<Bool> {
        Binding(
            get: { exportFormats.contains(format) },
            set: { enabled in if enabled { exportFormats.insert(format) } else { exportFormats.remove(format) } }
        )
    }

    private func clock(_ seconds: Double) -> String {
        let value = max(0, Int(seconds.rounded()))
        return String(format: "%02d:%02d:%02d", value / 3600, (value % 3600) / 60, value % 60)
    }
}

private struct SpeakerMerge: Identifiable {
    let source: String
    let target: String
    var id: String { "\(source)->\(target)" }
}

@MainActor
private final class TranscriptUndoCoordinator: ObservableObject {
    var apply: ((TranscriptDocument) -> Void)?

    func register(
        previous: TranscriptDocument,
        current: TranscriptDocument,
        name: String,
        with undoManager: UndoManager?
    ) {
        guard previous != current, let undoManager else { return }
        undoManager.registerUndo(withTarget: self) { target in
            target.apply?(previous)
            target.register(previous: current, current: previous, name: name, with: undoManager)
        }
        undoManager.setActionName(name)
    }
}

@MainActor
final class AudioPlaybackController: ObservableObject {
    @Published var currentTime = 0.0
    @Published var isPlaying = false
    @Published var loop = false
    @Published var rate: Float = 1
    private let player: AVPlayer
    private var monitor: Task<Void, Never>?
    private var range: ClosedRange<Double>?

    init(url: URL) { player = AVPlayer(url: url) }

    func play(start: Double, end: Double, loop: Bool = false) {
        self.loop = loop
        range = start...max(start, end)
        player.seek(to: CMTime(seconds: start, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero)
        player.playImmediately(atRate: rate)
        isPlaying = true
        monitorPlayback()
    }

    func toggle() {
        if isPlaying { player.pause(); isPlaying = false }
        else { player.playImmediately(atRate: rate); isPlaying = true; monitorPlayback() }
    }

    func seek(by seconds: Double) {
        let target = max(0, player.currentTime().seconds + seconds)
        player.seek(to: CMTime(seconds: target, preferredTimescale: 600))
    }

    func cycleRate() {
        let values: [Float] = [0.75, 1, 1.25, 1.5, 2]
        let next = ((values.firstIndex(of: rate) ?? 0) + 1) % values.count
        rate = values[next]
        if isPlaying { player.rate = rate }
    }

    func stop() {
        monitor?.cancel()
        player.pause()
        isPlaying = false
    }

    private func monitorPlayback() {
        monitor?.cancel()
        monitor = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 100_000_000)
                guard let self else { return }
                let time = player.currentTime().seconds
                currentTime = time.isFinite ? time : 0
                if let range, currentTime >= range.upperBound {
                    if loop {
                        await player.seek(to: CMTime(seconds: range.lowerBound, preferredTimescale: 600))
                        player.playImmediately(atRate: rate)
                    } else {
                        player.pause(); isPlaying = false; return
                    }
                }
            }
        }
    }
}
