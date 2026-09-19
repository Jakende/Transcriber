import Foundation

enum BookmarkStore {
    private static let outputKey = "transcription.outputFolderBookmark"
    private static let podcastKey = "transcription.podcastDownloadFolderBookmark"

    static func saveOutputFolder(_ url: URL?) {
        save(url, key: outputKey)
    }

    static func loadOutputFolder() -> URL? {
        load(key: outputKey)
    }

    static func savePodcastDownloadFolder(_ url: URL?) {
        save(url, key: podcastKey)
    }

    static func loadPodcastDownloadFolder() -> URL? {
        load(key: podcastKey)
    }

    private static func save(_ url: URL?, key: String) {
        guard let url else {
            UserDefaults.standard.removeObject(forKey: key)
            return
        }
        if let data = try? url.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil) {
            UserDefaults.standard.set(data, forKey: key)
        }
    }

    private static func load(key: String) -> URL? {
        guard let data = UserDefaults.standard.data(forKey: key) else { return nil }
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: data, options: .withSecurityScope, relativeTo: nil, bookmarkDataIsStale: &stale) else {
            return nil
        }
        if stale { save(url, key: key) }
        return url
    }
}
