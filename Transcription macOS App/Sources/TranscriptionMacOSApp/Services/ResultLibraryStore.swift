import Foundation

enum ResultLibraryStore {
    private static let archivedKey = "transcription.archivedResultIDs"

    static func archivedIDs() -> Set<String> {
        Set(UserDefaults.standard.stringArray(forKey: archivedKey) ?? [])
    }

    static func saveArchivedIDs(_ ids: Set<String>) {
        UserDefaults.standard.set(ids.sorted(), forKey: archivedKey)
    }
}
