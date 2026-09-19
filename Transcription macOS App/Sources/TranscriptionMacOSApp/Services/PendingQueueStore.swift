import Foundation

enum PendingQueueStore {
    private static let key = "transcription.pendingMediaPaths"
    private static let recordsKey = "transcription.pendingMediaRecords"

    static func load() -> [SelectedMediaFile] {
        if let data = UserDefaults.standard.data(forKey: recordsKey),
           let records = try? JSONDecoder().decode([SelectedMediaFile].self, from: data) {
            return records
        }
        return (UserDefaults.standard.stringArray(forKey: key) ?? [])
            .map { SelectedMediaFile(url: URL(fileURLWithPath: $0)) }
    }

    static func save(_ files: [SelectedMediaFile]) {
        let normalized = files.map {
            SelectedMediaFile(id: $0.id, url: $0.url.standardizedFileURL, podcastMetadata: $0.podcastMetadata)
        }
        if let data = try? JSONEncoder().encode(normalized) {
            UserDefaults.standard.set(data, forKey: recordsKey)
        }
        UserDefaults.standard.removeObject(forKey: key)
    }
}
