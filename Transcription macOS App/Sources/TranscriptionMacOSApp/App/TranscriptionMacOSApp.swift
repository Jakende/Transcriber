import SwiftUI

@main
struct TranscriptionMacOSApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var controller = TranscriptionController()

    var body: some Scene {
        WindowGroup("Transcription macOS") {
            ContentView()
                .environmentObject(controller)
                .frame(minWidth: 860, minHeight: 620)
        }
        .commands {
            CommandGroup(replacing: .newItem) { }
        }

        Settings {
            PodcastSettingsView()
                .frame(width: 560)
        }

        WindowGroup("Transkript bearbeiten", for: String.self) { $documentPath in
            if let documentPath {
                TranscriptEditorWindow(documentPath: documentPath)
                    .environmentObject(controller)
                    .frame(minWidth: 820, minHeight: 580)
            }
        }
        .defaultSize(width: 1050, height: 760)
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }
}
