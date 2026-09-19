import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct GlossaryManagerView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var terms = GlossaryStore.terms()
    @State private var newTerm = ""
    @State private var status = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Wörterbuch").font(.title2.bold())
                    Text("Diese Begriffe werden bei künftigen Transkriptionen als Whisper-Kontext verwendet.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button("Schließen") { dismiss() }
            }
            HStack {
                TextField("Name oder Fachbegriff", text: $newTerm)
                    .onSubmit(addTerm)
                Button("Hinzufügen", action: addTerm)
                    .buttonStyle(.borderedProminent)
                    .disabled(newTerm.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            if terms.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "character.book.closed").font(.system(size: 30)).foregroundStyle(.secondary)
                    Text("Noch keine bestätigten Begriffe").font(.headline)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List {
                    ForEach(terms, id: \.self) { term in
                        HStack {
                            Text(term).textSelection(.enabled)
                            Spacer()
                            Button(role: .destructive) {
                                terms.removeAll { $0 == term }
                                persist()
                            } label: { Image(systemName: "trash") }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            HStack {
                Button("Importieren …", action: importTerms)
                Button("Exportieren …", action: exportTerms).disabled(terms.isEmpty)
                Spacer()
                Text(status).font(.caption).foregroundStyle(.secondary)
                Text("\(terms.count) Begriffe").font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(20)
        .frame(minWidth: 620, minHeight: 480)
    }

    private func addTerm() {
        let value = newTerm.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return }
        terms.append(value)
        newTerm = ""
        persist()
    }

    private func persist() {
        GlossaryStore.save(terms)
        terms = GlossaryStore.terms()
        status = "Gespeichert"
    }

    private func importTerms() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.plainText, .commaSeparatedText]
        guard panel.runModal() == .OK, let url = panel.url,
              let content = try? String(contentsOf: url, encoding: .utf8) else {
            return
        }
        terms.append(contentsOf: content.components(separatedBy: .newlines).compactMap { line in
            let value = line.split(separator: ",", maxSplits: 1).first.map(String.init)?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            return value.isEmpty ? nil : value
        })
        persist()
        status = "Import abgeschlossen"
    }

    private func exportTerms() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.plainText]
        panel.nameFieldStringValue = "Transcription-Wörterbuch.txt"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try (terms.joined(separator: "\n") + "\n").write(to: url, atomically: true, encoding: .utf8)
            status = "Export abgeschlossen"
        } catch {
            status = "Fehler: \(error.localizedDescription)"
        }
    }
}
