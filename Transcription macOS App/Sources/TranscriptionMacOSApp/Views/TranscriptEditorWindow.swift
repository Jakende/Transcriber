import SwiftUI

struct TranscriptEditorWindow: View {
    @EnvironmentObject private var controller: TranscriptionController
    @State private var loaded: LoadedEditor?
    @State private var errorMessage: String?

    let documentPath: String

    var body: some View {
        Group {
            if let loaded {
                TranscriptEditorView(
                    initialDocument: loaded.document,
                    result: loaded.result,
                    runner: controller.runner
                ) { document in
                    controller.updateResult(loaded.result, document: document)
                }
            } else if let errorMessage {
                VStack(spacing: 10) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 32))
                        .foregroundStyle(.secondary)
                    Text("Transkript nicht verfügbar").font(.headline)
                    Text(errorMessage).foregroundStyle(.secondary)
                }
                .padding(30)
            } else {
                ProgressView("Transkript wird geöffnet …")
            }
        }
        .task(id: documentPath) { loadDocument() }
    }

    private func loadDocument() {
        let url = URL(fileURLWithPath: documentPath)
        do {
            let document = try controller.runner.loadDocument(at: url)
            let result = controller.results.first(where: { $0.documentURL.standardizedFileURL == url.standardizedFileURL })
                ?? TranscriptionResult(
                    sourceURL: URL(fileURLWithPath: document.sourcePath),
                    documentURL: url,
                    outputs: document.outputs,
                    speakerCount: SpeakerEditingSupport.labels(in: document).count,
                    segmentCount: document.segments.count
                )
            loaded = LoadedEditor(document: document, result: result)
            errorMessage = nil
        } catch {
            loaded = nil
            errorMessage = error.localizedDescription
        }
    }
}

private struct LoadedEditor {
    let document: TranscriptDocument
    let result: TranscriptionResult
}
