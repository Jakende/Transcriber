import AppKit
import SwiftUI

struct PodcastSettingsView: View {
    @State private var key = ""
    @State private var secret = ""
    @State private var folder = BookmarkStore.loadPodcastDownloadFolder()
    @State private var status = ""
    @State private var isTesting = false
    @AppStorage("transcription.podcastAutoTranscribe") private var autoTranscribe = true

    var body: some View {
        Form {
            Section("PodcastIndex") {
                SecureField("PODCAST_INDEX_KEY", text: $key)
                SecureField("PODCAST_INDEX_SECRET", text: $secret)
                Text("Leere Felder verändern bereits gespeicherte Werte nicht. Ohne Schlüsselbundwert werden gleichnamige Umgebungsvariablen verwendet.")
                    .font(.caption).foregroundStyle(.secondary)
                HStack {
                    Button("Im Schlüsselbund speichern") { saveCredentials() }
                    Button("Verbindung testen") { testConnection() }.disabled(isTesting)
                    if isTesting { ProgressView().controlSize(.small) }
                }
            }
            Section("Medien-Downloads") {
                LabeledContent("Downloadordner") {
                    HStack {
                        Text(folder?.path ?? "Nicht gewählt").foregroundStyle(.secondary).lineLimit(1)
                        Button("Wählen …") { chooseFolder() }
                    }
                }
                Toggle("Nach Download transkribieren", isOn: $autoTranscribe)
            }
            if !status.isEmpty { Text(status).font(.caption).textSelection(.enabled) }
        }
        .formStyle(.grouped)
        .padding(.vertical, 12)
    }

    private func saveCredentials() {
        do {
            if !key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                try PodcastCredentialStore.save(key, for: PodcastCredentialStore.keyName)
            }
            if !secret.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                try PodcastCredentialStore.save(secret, for: PodcastCredentialStore.secretName)
            }
            key = ""; secret = ""
            status = "Zugangsdaten sicher im macOS-Schlüsselbund gespeichert."
        } catch { status = error.localizedDescription }
    }

    private func testConnection() {
        saveCredentials()
        guard let credentials = PodcastCredentialStore.credentials() else {
            status = PodcastServiceError.missingCredentials.localizedDescription
            return
        }
        isTesting = true
        Task {
            do {
                try await PodcastIndexClient(credentials: credentials).testConnection()
                status = "Verbindung zu PodcastIndex erfolgreich."
            } catch { status = error.localizedDescription }
            isTesting = false
        }
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false; panel.canChooseDirectories = true; panel.canCreateDirectories = true
        panel.message = "Ordner für heruntergeladene Podcast- und YouTube-Medien wählen"
        if panel.runModal() == .OK {
            folder = panel.url
            BookmarkStore.savePodcastDownloadFolder(panel.url)
        }
    }
}

struct MediaBrowserView: View {
    private enum Source: String, CaseIterable, Identifiable {
        case rss = "RSS-Feed"
        case index = "PodcastIndex"
        case youtube = "YouTube"
        var id: String { rawValue }
    }

    @Environment(\.dismiss) private var dismiss
    @StateObject private var podcastCoordinator = PodcastDownloadCoordinator()
    @StateObject private var youtubeCoordinator = YouTubeDownloadCoordinator()
    @State private var source = Source.rss
    @State private var rssURL = ""
    @State private var searchTerm = ""
    @State private var searchResults: [PodcastIndexSearchResult] = []
    @State private var feed: PodcastFeed?
    @State private var selected = Set<String>()
    @State private var episodeSearch = ""
    @State private var folder = BookmarkStore.loadPodcastDownloadFolder()
    @State private var message = ""
    @State private var isLoading = false
    @State private var completed = false
    @State private var snapshot: TranscriptionSettings?
    @State private var youtubeURL = ""
    @State private var keepYouTubeVideo = false
    @AppStorage("transcription.podcastAutoTranscribe") private var autoTranscribe = true

    let settingsProvider: () -> TranscriptionSettings
    let onComplete: ([SelectedMediaFile], Bool, TranscriptionSettings) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Medien laden").font(.title2.bold())
                Spacer()
                Button("Schließen") { dismiss() }.disabled(isRunning)
            }
            Picker("Quelle", selection: $source) {
                ForEach(Source.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .disabled(isRunning)

            switch source {
            case .rss:
                rssEntry
            case .index:
                indexSearch
            case .youtube:
                youtubeEntry
            }
            Divider()
            if source == .youtube {
                youtubeState
            } else if let feed {
                episodeList(feed)
            } else {
                emptyState
            }
            Divider()
            downloadBar
        }
        .padding(22)
        .frame(minWidth: 820, minHeight: 650)
        .interactiveDismissDisabled(isRunning)
        .onChange(of: youtubeURL) { _ in youtubeCoordinator.reset() }
    }

    private var isRunning: Bool {
        podcastCoordinator.isRunning || youtubeCoordinator.isRunning || youtubeCoordinator.isInspecting
    }

    private var rssEntry: some View {
        HStack {
            TextField("https://example.org/podcast.xml", text: $rssURL)
                .textFieldStyle(.roundedBorder)
                .onSubmit { loadRSS() }
            Button("Feed laden") { loadRSS() }.disabled(isLoading || rssURL.isEmpty)
        }
    }

    private var indexSearch: some View {
        VStack(spacing: 8) {
            HStack {
                TextField("Podcast suchen", text: $searchTerm).textFieldStyle(.roundedBorder).onSubmit { search() }
                Button("Suchen") { search() }.disabled(isLoading || searchTerm.isEmpty)
            }
            if !searchResults.isEmpty {
                List(searchResults) { result in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(result.title).font(.headline)
                            Text([result.author, result.language].compactMap { $0 }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button("Feed öffnen") { loadSearchResult(result) }
                    }
                }.frame(height: 150)
            }
        }
    }

    private var youtubeEntry: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                TextField("https://www.youtube.com/watch?v=…", text: $youtubeURL)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { inspectYouTube() }
                Button("Video prüfen") { inspectYouTube() }
                    .disabled(youtubeURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isRunning)
            }
            Toggle("Videodatei zusätzlich speichern", isOn: $keepYouTubeVideo)
                .toggleStyle(.checkbox)
                .disabled(isRunning)
            Text("Das Audio wird immer als MP3 gespeichert. Optional bleibt zusätzlich die Videodatei im Downloadordner erhalten. Wiedergabelisten werden nicht automatisch geladen.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var youtubeState: some View {
        VStack(spacing: 12) {
            if youtubeCoordinator.isInspecting {
                ProgressView("Videoinformationen werden geladen …")
            } else if let info = youtubeCoordinator.info {
                Image(systemName: "play.rectangle")
                    .font(.system(size: 30))
                    .foregroundStyle(.secondary)
                Text(info.title).font(.headline).multilineTextAlignment(.center).lineLimit(3)
                Text([info.channel, info.durationSeconds.map(durationText)].compactMap { $0 }.joined(separator: " · "))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if youtubeCoordinator.isRunning {
                    ProgressView(value: youtubeCoordinator.progress).frame(maxWidth: 360)
                    Text(youtubeCoordinator.phase).font(.caption).foregroundStyle(.secondary)
                }
            } else {
                Image(systemName: "play.rectangle")
                    .font(.system(size: 30))
                    .foregroundStyle(.secondary)
                Text("Noch kein YouTube-Video geprüft").font(.headline)
            }
            if let error = youtubeCoordinator.errorMessage {
                Text(error).font(.caption).foregroundStyle(.red).textSelection(.enabled)
            } else if !message.isEmpty {
                Text(message).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding()
    }

    private var emptyState: some View {
        VStack(spacing: 8) {
            if isLoading { ProgressView() }
            Image(systemName: "dot.radiowaves.left.and.right").font(.system(size: 30)).foregroundStyle(.secondary)
            Text(isLoading ? "Feed wird geladen …" : "Noch kein Podcast geladen").font(.headline)
            if !message.isEmpty { Text(message).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
        }.frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func episodeList(_ feed: PodcastFeed) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                VStack(alignment: .leading) {
                    Text(feed.showTitle).font(.headline)
                    Text("\(feed.episodes.count) Folgen · \(feed.feedURL.absoluteString)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                TextField("Folgen durchsuchen", text: $episodeSearch).textFieldStyle(.roundedBorder).frame(width: 230)
            }
            List {
                ForEach(filteredEpisodes(feed), id: \.id) { (episode: PodcastEpisode) in
                    HStack(alignment: .top, spacing: 10) {
                    Toggle("", isOn: Binding(
                        get: { selected.contains(episode.id) },
                        set: { value in if value { selected.insert(episode.id) } else { selected.remove(episode.id) } }
                    )).labelsHidden().toggleStyle(.checkbox).disabled(episode.unavailableReason != nil || podcastCoordinator.isRunning)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(episode.title ?? "Unbenannte Folge").lineLimit(2)
                        HStack {
                            if let date = episode.publishedAt { Text(Self.dateFormatter.string(from: date)) }
                            if let duration = episode.durationSeconds { Text(durationText(duration)) }
                            if let reason = episode.unavailableReason { Text(reason).foregroundStyle(.red) }
                        }.font(.caption).foregroundStyle(.secondary)
                        if let state = podcastCoordinator.states.first(where: { $0.id == episode.id }) {
                            HStack {
                                ProgressView(value: state.progress).frame(width: 120)
                                Text(state.error ?? state.phase).font(.caption).foregroundStyle(state.error == nil ? Color.secondary : Color.red)
                            }
                        }
                    }
                    }.padding(.vertical, 3)
                }
            }
        }
    }

    private var downloadBar: some View {
        VStack(alignment: .leading, spacing: 10) {
            if !message.isEmpty && feed != nil && source != .youtube { Text(message).font(.caption).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(folder?.path ?? "Kein Downloadordner gewählt").lineLimit(1)
                    Text(source == .youtube ? "Audio wird als MP3 gespeichert." : "Nur ausgewählte Folgen werden geladen.")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Button("Ordner wählen …") { chooseFolder() }.disabled(isRunning)
                Toggle("Danach transkribieren", isOn: $autoTranscribe).disabled(isRunning)
                if podcastCoordinator.isRunning {
                    Button("Abbrechen", role: .destructive) { podcastCoordinator.cancel() }
                } else if youtubeCoordinator.isRunning {
                    Button("Abbrechen", role: .destructive) { youtubeCoordinator.cancel() }
                } else if source == .youtube {
                    Button(completed ? "Erneut herunterladen" : "YouTube-Audio herunterladen") { downloadYouTube() }
                        .buttonStyle(.borderedProminent)
                        .disabled(youtubeCoordinator.info == nil || folder == nil)
                } else {
                    Button(completed ? "Erneut herunterladen" : "Auswahl herunterladen") { downloadSelection() }
                        .buttonStyle(.borderedProminent)
                        .disabled(selected.isEmpty || folder == nil || feed == nil)
                }
            }
        }
    }

    private func loadRSS() {
        let value = rssURL.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let url = URL(string: value) else { message = PodcastServiceError.invalidURL.localizedDescription; return }
        loadFeed(url: url, indexID: nil)
    }

    private func search() {
        guard let credentials = PodcastCredentialStore.credentials() else { message = PodcastServiceError.missingCredentials.localizedDescription; return }
        isLoading = true; message = ""
        Task {
            do { searchResults = try await PodcastIndexClient(credentials: credentials).search(searchTerm) }
            catch { message = error.localizedDescription }
            isLoading = false
        }
    }

    private func loadSearchResult(_ result: PodcastIndexSearchResult) {
        guard let url = URL(string: result.url) else { message = PodcastServiceError.invalidURL.localizedDescription; return }
        loadFeed(url: url, indexID: result.id)
    }

    private func loadFeed(url: URL, indexID: Int?) {
        isLoading = true; message = ""; selected.removeAll(); feed = nil
        Task {
            do { feed = try await PodcastFeedLoader.load(url: url, podcastIndexFeedID: indexID) }
            catch { message = error.localizedDescription }
            isLoading = false
        }
    }

    private func downloadSelection() {
        guard let feed, let folder else { message = PodcastDownloadError.noDestination.localizedDescription; return }
        let episodes = feed.episodes.filter { selected.contains($0.id) && $0.unavailableReason == nil }
        let settings = settingsProvider()
        snapshot = settings
        completed = false; message = ""
        Task {
            let files = await podcastCoordinator.download(feed: feed, episodes: episodes, to: folder)
            if !files.isEmpty { onComplete(files, autoTranscribe, settings) }
            completed = true
            let failures = episodes.count - files.count
            message = failures == 0 ? "" : "\(files.count) abgeschlossen, \(failures) fehlgeschlagen."
        }
    }

    private func inspectYouTube() {
        completed = false
        message = ""
        Task { await youtubeCoordinator.inspect(youtubeURL) }
    }

    private func downloadYouTube() {
        guard let folder else { return }
        let settings = settingsProvider()
        snapshot = settings
        completed = false
        message = ""
        Task {
            let result = await youtubeCoordinator.download(
                rawURL: youtubeURL,
                info: youtubeCoordinator.info,
                keepVideo: keepYouTubeVideo,
                to: folder
            )
            if let result {
                onComplete([result.audioFile], autoTranscribe, settings)
                if let videoURL = result.videoURL {
                    message = "MP3 und Video gespeichert: \(videoURL.lastPathComponent)"
                } else {
                    message = "MP3 gespeichert: \(result.audioFile.name)"
                }
            }
            completed = result != nil
        }
    }

    private func chooseFolder() {
        let panel = NSOpenPanel(); panel.canChooseFiles = false; panel.canChooseDirectories = true; panel.canCreateDirectories = true
        if panel.runModal() == .OK { folder = panel.url; BookmarkStore.savePodcastDownloadFolder(panel.url) }
    }

    private func filteredEpisodes(_ feed: PodcastFeed) -> [PodcastEpisode] {
        let query = episodeSearch.trimmingCharacters(in: .whitespacesAndNewlines)
        return query.isEmpty ? feed.episodes : feed.episodes.filter { ($0.title ?? "").localizedCaseInsensitiveContains(query) }
    }

    private func durationText(_ seconds: Int) -> String {
        seconds >= 3600 ? String(format: "%d:%02d:%02d", seconds / 3600, seconds % 3600 / 60, seconds % 60) : String(format: "%d:%02d", seconds / 60, seconds % 60)
    }

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter(); formatter.dateStyle = .medium; formatter.timeStyle = .none; return formatter
    }()
}
