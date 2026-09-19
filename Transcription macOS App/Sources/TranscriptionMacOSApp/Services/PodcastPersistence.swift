import Foundation
import Security

enum PodcastCredentialError: LocalizedError {
    case keychain(OSStatus)

    var errorDescription: String? {
        switch self {
        case .keychain(let status):
            return "Der PodcastIndex-Zugang konnte nicht im Schlüsselbund gespeichert werden (Fehler \(status))."
        }
    }
}

struct PodcastIndexCredentials: Equatable {
    let key: String
    let secret: String
}

enum PodcastCredentialStore {
    static let keyName = "PODCAST_INDEX_KEY"
    static let secretName = "PODCAST_INDEX_SECRET"
    private static let service = "de.jakende.transcription-macos.podcastindex"

    static func credentials(environment: [String: String] = ProcessInfo.processInfo.environment) -> PodcastIndexCredentials? {
        resolve(keychainKey: value(for: keyName), keychainSecret: value(for: secretName), environment: environment)
    }

    static func resolve(keychainKey: String?, keychainSecret: String?, environment: [String: String]) -> PodcastIndexCredentials? {
        let key = nonempty(keychainKey) ?? nonempty(environment[keyName])
        let secret = nonempty(keychainSecret) ?? nonempty(environment[secretName])
        guard let key, let secret else { return nil }
        return PodcastIndexCredentials(key: key, secret: secret)
    }

    static func value(for account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data,
              let string = String(data: data, encoding: .utf8) else { return nil }
        return nonempty(string)
    }

    static func save(_ value: String, for account: String) throws {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            delete(account)
            return
        }
        let match: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        let attributes: [String: Any] = [kSecValueData as String: Data(trimmed.utf8)]
        let updateStatus = SecItemUpdate(match as CFDictionary, attributes as CFDictionary)
        if updateStatus == errSecSuccess { return }
        guard updateStatus == errSecItemNotFound else { throw PodcastCredentialError.keychain(updateStatus) }
        var insert = match
        insert[kSecValueData as String] = Data(trimmed.utf8)
        insert[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        let insertStatus = SecItemAdd(insert as CFDictionary, nil)
        guard insertStatus == errSecSuccess else { throw PodcastCredentialError.keychain(insertStatus) }
    }

    private static func delete(_ account: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }

    private static func nonempty(_ value: String?) -> String? {
        guard let value else { return nil }
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

enum PodcastDownloadRegistry {
    private static var fileURL: URL? {
        guard let base = try? FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        ) else { return nil }
        let directory = base.appendingPathComponent("Transcription macOS", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory.appendingPathComponent("PodcastDownloads.json")
    }

    static func metadata(for url: URL) -> PodcastMetadata? {
        records()[url.standardizedFileURL.path]
    }

    static func register(_ file: SelectedMediaFile) {
        guard let metadata = file.podcastMetadata, let fileURL else { return }
        var stored = records()
        stored[file.url.standardizedFileURL.path] = metadata
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        if let data = try? encoder.encode(stored) {
            try? data.write(to: fileURL, options: .atomic)
        }
    }

    private static func records() -> [String: PodcastMetadata] {
        guard let fileURL,
              let data = try? Data(contentsOf: fileURL),
              let value = try? JSONDecoder().decode([String: PodcastMetadata].self, from: data) else { return [:] }
        return value
    }
}
