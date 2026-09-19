import AppKit
import CryptoKit
import Foundation

enum PodcastServiceError: LocalizedError {
    case missingCredentials
    case invalidURL
    case insecureResource
    case responseTooLarge
    case unauthorized
    case rateLimited
    case server(Int)
    case invalidJSON
    case invalidFeed(String)
    case missingEnclosure

    var errorDescription: String? {
        switch self {
        case .missingCredentials: return "PodcastIndex-Zugangsdaten fehlen. Bitte in den Einstellungen hinterlegen."
        case .invalidURL: return "Die eingegebene Adresse ist ungültig."
        case .insecureResource: return "Die Ressource ist nicht sicher: Zulässig ist HTTPS oder eine direkte Weiterleitung von HTTP auf HTTPS."
        case .responseTooLarge: return "Die Serverantwort überschreitet die zulässige Größe."
        case .unauthorized: return "PodcastIndex hat die Zugangsdaten abgelehnt (401)."
        case .rateLimited: return "PodcastIndex hat zu viele Anfragen gemeldet (429). Bitte später erneut versuchen."
        case .server(let status): return "Der Server antwortete mit HTTP \(status)."
        case .invalidJSON: return "Die Antwort von PodcastIndex enthält kein lesbares JSON."
        case .invalidFeed(let reason): return "Der RSS-Feed konnte nicht gelesen werden: \(reason)"
        case .missingEnclosure: return "Der Feed enthält keine auswählbaren Folgen mit Audio-Enclosure."
        }
    }
}

struct PodcastIndexSearchResult: Identifiable, Hashable, Decodable {
    let id: Int
    let title: String
    let url: String
    let author: String?
    let description: String?
    let image: String?
    let language: String?
    let episodeCount: Int?

    enum CodingKeys: String, CodingKey {
        case id, title, url, author, description, image, language
        case episodeCount = "episodeCount"
    }
}

struct PodcastFeed: Hashable {
    let feedURL: URL
    let podcastIndexFeedID: Int?
    let showTitle: String
    let author: String?
    let publisher: String?
    let language: String?
    let imageURL: String?
    let categories: [String]
    let description: String?
    let rawDescription: String?
    let episodes: [PodcastEpisode]
}

struct PodcastEpisode: Identifiable, Hashable {
    let id: String
    let title: String?
    let audioURL: URL?
    let enclosureType: String?
    let guid: String?
    let publishedAt: Date?
    let author: String?
    let publisher: String?
    let language: String?
    let episodeNumber: Int?
    let seasonNumber: Int?
    let episodeType: String?
    let episodeURL: String?
    let durationSeconds: Int?
    let explicit: Bool?
    let imageURL: String?
    let categories: [String]
    let description: String?
    let rawDescription: String?

    var unavailableReason: String? {
        if title?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty != false { return "Titel fehlt" }
        guard let audioURL else { return "Audio-Enclosure fehlt" }
        guard ["https", "http"].contains(audioURL.scheme?.lowercased() ?? "") else { return "Unsichere Audio-Adresse" }
        return nil
    }
}

struct PodcastIndexClient {
    static let apiRoot = URL(string: "https://api.podcastindex.org/api/1.0")!
    let credentials: PodcastIndexCredentials
    var now: () -> Date = Date.init
    var userAgent: String = PodcastUserAgent.value

    func authenticationHeaders(date: Date? = nil) -> [String: String] {
        let timestamp = String(Int((date ?? now()).timeIntervalSince1970))
        let material = Data((credentials.key + credentials.secret + timestamp).utf8)
        let signature = Insecure.SHA1.hash(data: material).map { String(format: "%02x", $0) }.joined()
        return [
            "User-Agent": userAgent,
            "X-Auth-Key": credentials.key,
            "X-Auth-Date": timestamp,
            "Authorization": signature,
        ]
    }

    func search(_ term: String) async throws -> [PodcastIndexSearchResult] {
        let query = term.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return [] }
        var components = URLComponents(url: Self.apiRoot.appendingPathComponent("search/byterm"), resolvingAgainstBaseURL: false)!
        components.queryItems = [URLQueryItem(name: "q", value: query), URLQueryItem(name: "max", value: "40")]
        guard let url = components.url else { throw PodcastServiceError.invalidURL }
        var request = URLRequest(url: url)
        authenticationHeaders().forEach { request.setValue($0.value, forHTTPHeaderField: $0.key) }
        let (data, response) = try await SecureHTTPClient.data(for: request, limit: 5_000_000)
        switch response.statusCode {
        case 200..<300: break
        case 401: throw PodcastServiceError.unauthorized
        case 429: throw PodcastServiceError.rateLimited
        default: throw PodcastServiceError.server(response.statusCode)
        }
        struct Envelope: Decodable { let feeds: [PodcastIndexSearchResult] }
        guard let decoded = try? JSONDecoder().decode(Envelope.self, from: data) else { throw PodcastServiceError.invalidJSON }
        return decoded.feeds
    }

    func testConnection() async throws {
        _ = try await search("test")
    }
}

enum PodcastUserAgent {
    static var value: String {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "development"
        return "Transcription-macOS/\(version)"
    }
}

enum SecureHTTPClient {
    static func isAllowedInitialURL(_ url: URL) -> Bool {
        ["https", "http"].contains(url.scheme?.lowercased() ?? "")
    }

    static func isAllowedRedirect(to url: URL) -> Bool {
        url.scheme?.lowercased() == "https"
    }

    static func data(for request: URLRequest, limit: Int) async throws -> (Data, HTTPURLResponse) {
        guard let url = request.url, isAllowedInitialURL(url) else {
            throw PodcastServiceError.insecureResource
        }
        return try await BoundedDataLoader(limit: limit).start(request: request)
    }
}

private final class BoundedDataLoader: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let limit: Int
    private var data = Data()
    private var response: HTTPURLResponse?
    private var continuation: CheckedContinuation<(Data, HTTPURLResponse), Error>?
    private var session: URLSession?
    private var completed = false

    init(limit: Int) { self.limit = limit }

    func start(request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        try await withCheckedThrowingContinuation { continuation in
            self.continuation = continuation
            let configuration = URLSessionConfiguration.ephemeral
            configuration.timeoutIntervalForRequest = 30
            configuration.timeoutIntervalForResource = 60
            let session = URLSession(configuration: configuration, delegate: self, delegateQueue: nil)
            self.session = session
            session.dataTask(with: request).resume()
        }
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse, completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        guard let http = response as? HTTPURLResponse else {
            finish(.failure(PodcastServiceError.invalidFeed("Keine HTTP-Antwort")))
            completionHandler(.cancel)
            return
        }
        guard http.url?.scheme?.lowercased() == "https" else {
            finish(.failure(PodcastServiceError.insecureResource)); completionHandler(.cancel); return
        }
        if response.expectedContentLength > Int64(limit) {
            finish(.failure(PodcastServiceError.responseTooLarge)); completionHandler(.cancel); return
        }
        self.response = http
        completionHandler(.allow)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive newData: Data) {
        guard !completed else { return }
        guard data.count + newData.count <= limit else {
            finish(.failure(PodcastServiceError.responseTooLarge))
            dataTask.cancel()
            return
        }
        data.append(newData)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        defer { session.finishTasksAndInvalidate() }
        if let error, !completed { finish(.failure(error)); return }
        guard let response else { finish(.failure(PodcastServiceError.invalidFeed("Leere Serverantwort"))); return }
        finish(.success((data, response)))
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(request.url.map(SecureHTTPClient.isAllowedRedirect(to:)) == true ? request : nil)
    }

    private func finish(_ result: Result<(Data, HTTPURLResponse), Error>) {
        guard !completed, let continuation else { return }
        completed = true
        self.continuation = nil
        continuation.resume(with: result)
    }
}

enum PodcastFeedLoader {
    static func load(url: URL, podcastIndexFeedID: Int? = nil) async throws -> PodcastFeed {
        var request = URLRequest(url: url)
        request.setValue(PodcastUserAgent.value, forHTTPHeaderField: "User-Agent")
        let (data, response) = try await SecureHTTPClient.data(for: request, limit: 12_000_000)
        guard (200..<300).contains(response.statusCode) else { throw PodcastServiceError.server(response.statusCode) }
        return try PodcastRSSParser.parse(data: data, feedURL: response.url ?? url, podcastIndexFeedID: podcastIndexFeedID)
    }
}

enum PodcastRSSParser {
    static func parse(data: Data, feedURL: URL, podcastIndexFeedID: Int? = nil) throws -> PodcastFeed {
        let delegate = RSSDelegate(feedURL: feedURL, podcastIndexFeedID: podcastIndexFeedID)
        let parser = XMLParser(data: data)
        parser.shouldProcessNamespaces = true
        parser.shouldResolveExternalEntities = false
        parser.externalEntityResolvingPolicy = .never
        parser.delegate = delegate
        guard parser.parse(), let feed = delegate.feed else {
            throw PodcastServiceError.invalidFeed(parser.parserError?.localizedDescription ?? delegate.failure ?? "Unbekannter XML-Fehler")
        }
        guard !feed.episodes.isEmpty else { throw PodcastServiceError.missingEnclosure }
        return feed
    }
}

private final class RSSDelegate: NSObject, XMLParserDelegate {
    let feedURL: URL
    let podcastIndexFeedID: Int?
    var feed: PodcastFeed?
    var failure: String?
    private var stack: [String] = []
    private var text = ""
    private var show = ShowBuilder()
    private var episode: EpisodeBuilder?
    private var episodes: [PodcastEpisode] = []

    init(feedURL: URL, podcastIndexFeedID: Int?) {
        self.feedURL = feedURL
        self.podcastIndexFeedID = podcastIndexFeedID
    }

    func parser(_ parser: XMLParser, didStartElement elementName: String, namespaceURI: String?, qualifiedName qName: String?, attributes attributeDict: [String: String] = [:]) {
        let name = normalized(elementName, qName)
        stack.append(name)
        text = ""
        if name == "item" { episode = EpisodeBuilder() }
        if name == "enclosure", episode != nil {
            episode?.audioURL = attributeDict["url"].flatMap(URL.init(string:))
            episode?.enclosureType = clean(attributeDict["type"])
        }
        if name == "image", let href = attributeDict["href"] ?? attributeDict["url"] {
            if episode != nil { episode?.imageURL = href } else { show.imageURL = href }
        }
        if name == "category", let category = clean(attributeDict["text"]) {
            if episode != nil { episode?.categories.append(category) } else { show.categories.append(category) }
        }
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) { text += string }
    func parser(_ parser: XMLParser, foundCDATA CDATABlock: Data) { text += String(data: CDATABlock, encoding: .utf8) ?? "" }

    func parser(_ parser: XMLParser, didEndElement elementName: String, namespaceURI: String?, qualifiedName qName: String?) {
        let name = normalized(elementName, qName)
        let value = clean(text)
        let path = stack.joined(separator: "/")
        if var item = episode {
            switch name {
            case "title": item.title = item.title ?? value
            case "guid": item.guid = item.guid ?? value
            case "pubdate", "date": item.publishedAt = item.publishedAt ?? parsePodcastDate(value)
            case "link": item.episodeURL = item.episodeURL ?? value
            case "language": item.language = item.language ?? value
            case "duration": item.durationSeconds = item.durationSeconds ?? parseDuration(value)
            case "episode": item.episodeNumber = item.episodeNumber ?? value.flatMap(Int.init)
            case "season": item.seasonNumber = item.seasonNumber ?? value.flatMap(Int.init)
            case "episodetype": item.episodeType = item.episodeType ?? value
            case "explicit": item.explicit = item.explicit ?? parseExplicit(value)
            case "description", "summary", "encoded": item.description = item.description ?? value
            case "author": item.author = item.author ?? value
            case "creator": item.creator = item.creator ?? value
            case "category": if let value { item.categories.append(value) }
            default: break
            }
            episode = item
            if name == "item" {
                episodes.append(item.build(show: show))
                episode = nil
            }
        } else {
            switch name {
            case "title" where path.hasSuffix("channel/title"): show.title = show.title ?? value
            case "language": show.language = show.language ?? value
            case "author": show.author = show.author ?? value
            case "creator": show.creator = show.creator ?? value
            case "managingeditor": show.publisher = show.publisher ?? value
            case "name" where path.contains("owner"): show.publisher = show.publisher ?? value
            case "description", "summary", "encoded": show.description = show.description ?? value
            case "url" where path.contains("image"): show.imageURL = show.imageURL ?? value
            case "category": if let value { show.categories.append(value) }
            default: break
            }
        }
        if !stack.isEmpty { stack.removeLast() }
        text = ""
    }

    func parserDidEndDocument(_ parser: XMLParser) {
        guard let title = clean(show.title) else { failure = "Titel der Show fehlt"; return }
        let sorted = episodes.sorted { ($0.publishedAt ?? .distantPast) > ($1.publishedAt ?? .distantPast) }
        feed = PodcastFeed(
            feedURL: feedURL,
            podcastIndexFeedID: podcastIndexFeedID,
            showTitle: title,
            author: show.author ?? show.creator,
            publisher: show.publisher,
            language: show.language,
            imageURL: show.imageURL,
            categories: unique(show.categories),
            description: show.description.map(plainText),
            rawDescription: show.description,
            episodes: sorted
        )
    }

    private func normalized(_ elementName: String, _ qualifiedName: String?) -> String {
        (qualifiedName ?? elementName).split(separator: ":").last.map(String.init)?.lowercased() ?? elementName.lowercased()
    }
}

private struct ShowBuilder {
    var title: String?
    var author: String?
    var creator: String?
    var publisher: String?
    var language: String?
    var imageURL: String?
    var categories: [String] = []
    var description: String?
}

private struct EpisodeBuilder {
    var title: String?
    var audioURL: URL?
    var enclosureType: String?
    var guid: String?
    var publishedAt: Date?
    var author: String?
    var creator: String?
    var language: String?
    var episodeNumber: Int?
    var seasonNumber: Int?
    var episodeType: String?
    var episodeURL: String?
    var durationSeconds: Int?
    var explicit: Bool?
    var imageURL: String?
    var categories: [String] = []
    var description: String?

    func build(show: ShowBuilder) -> PodcastEpisode {
        let identity = guid ?? audioURL?.absoluteString ?? UUID().uuidString
        return PodcastEpisode(
            id: identity,
            title: title,
            audioURL: audioURL,
            enclosureType: enclosureType,
            guid: guid,
            publishedAt: publishedAt,
            author: author ?? creator ?? show.author ?? show.creator,
            publisher: show.publisher,
            language: language ?? show.language,
            episodeNumber: episodeNumber,
            seasonNumber: seasonNumber,
            episodeType: episodeType,
            episodeURL: episodeURL,
            durationSeconds: durationSeconds,
            explicit: explicit,
            imageURL: imageURL ?? show.imageURL,
            categories: unique(categories.isEmpty ? show.categories : categories),
            description: description.map(plainText),
            rawDescription: description
        )
    }
}

private func clean(_ value: String?) -> String? {
    guard let value else { return nil }
    let normalized = value.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
        .trimmingCharacters(in: .whitespacesAndNewlines)
    return normalized.isEmpty ? nil : normalized
}

private func plainText(_ html: String) -> String {
    guard let data = html.data(using: .utf8),
          let attributed = try? NSAttributedString(
            data: data,
            options: [.documentType: NSAttributedString.DocumentType.html, .characterEncoding: String.Encoding.utf8.rawValue],
            documentAttributes: nil
          ) else { return html.replacingOccurrences(of: "<[^>]+>", with: " ", options: .regularExpression) }
    return attributed.string.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression).trimmingCharacters(in: .whitespacesAndNewlines)
}

private func unique(_ values: [String]) -> [String] {
    var seen = Set<String>()
    return values.compactMap(clean).filter { seen.insert($0).inserted }
}

private func parseExplicit(_ value: String?) -> Bool? {
    guard let value = value?.lowercased() else { return nil }
    if ["yes", "true", "explicit", "1"].contains(value) { return true }
    if ["no", "false", "clean", "0"].contains(value) { return false }
    return nil
}

private func parseDuration(_ value: String?) -> Int? {
    guard let value else { return nil }
    if let direct = Int(value) { return direct }
    let parts = value.split(separator: ":").compactMap { Int($0) }
    if parts.count == 3 { return parts[0] * 3600 + parts[1] * 60 + parts[2] }
    if parts.count == 2 { return parts[0] * 60 + parts[1] }
    return nil
}

private func parsePodcastDate(_ value: String?) -> Date? {
    guard let value else { return nil }
    let iso = ISO8601DateFormatter()
    if let date = iso.date(from: value) { return date }
    for format in ["EEE, dd MMM yyyy HH:mm:ss Z", "EEE, d MMM yyyy HH:mm:ss Z", "dd MMM yyyy HH:mm:ss Z", "yyyy-MM-dd HH:mm:ss Z"] {
        let parser = DateFormatter()
        parser.locale = Locale(identifier: "en_US_POSIX")
        parser.dateFormat = format
        if let date = parser.date(from: value) { return date }
    }
    return nil
}
