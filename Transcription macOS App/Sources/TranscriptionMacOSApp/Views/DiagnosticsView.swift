import AppKit
import SwiftUI

struct DiagnosticsView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var report = "Prüfe Installation …"
    let runner: PythonTranscriptionRunner

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("Systemprüfung").font(.title2.bold())
                Spacer()
                Button("Schließen") { dismiss() }
            }
            ScrollView {
                Text(report)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
            }
            .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(.separator, lineWidth: 1))
            HStack {
                Button("Bericht kopieren") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(report, forType: .string)
                }
                Spacer()
            }
        }
        .padding(20)
        .frame(minWidth: 680, minHeight: 430)
        .task { report = runner.diagnosticReport() }
    }
}
