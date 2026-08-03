import Foundation

enum BookmarkStore {
    private static let key = "transcription.outputFolderBookmark"

    static func saveOutputFolder(_ url: URL?) {
        guard let url else {
            UserDefaults.standard.removeObject(forKey: key)
            return
        }
        if let data = try? url.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil) {
            UserDefaults.standard.set(data, forKey: key)
        }
    }

    static func loadOutputFolder() -> URL? {
        guard let data = UserDefaults.standard.data(forKey: key) else { return nil }
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: data, options: .withSecurityScope, relativeTo: nil, bookmarkDataIsStale: &stale) else {
            return nil
        }
        if stale { saveOutputFolder(url) }
        return url
    }
}
