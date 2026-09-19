import CryptoKit
import Foundation
import XCTest
@testable import TranscriptionMacOSApp

final class PodcastSupportTests: XCTestCase {
    func testPodcastIndexHeadersUseDeterministicSHA1() {
        let date = Date(timeIntervalSince1970: 1_700_000_000)
        let client = PodcastIndexClient(
            credentials: PodcastIndexCredentials(key: "key", secret: "secret"),
            now: { date },
            userAgent: "Tests/1"
        )
        let headers = client.authenticationHeaders()
        XCTAssertEqual(headers["X-Auth-Key"], "key")
        XCTAssertEqual(headers["X-Auth-Date"], "1700000000")
        XCTAssertEqual(headers["Authorization"], "abaf71c02050c31e4d4e6b08c1625173af0445ba")
        XCTAssertEqual(headers["User-Agent"], "Tests/1")
    }

    func testCredentialResolverPrefersKeychainAndFallsBackToEnvironment() {
        XCTAssertEqual(
            PodcastCredentialStore.resolve(
                keychainKey: nil,
                keychainSecret: nil,
                environment: ["PODCAST_INDEX_KEY": "env-key", "PODCAST_INDEX_SECRET": "env-secret"]
            ),
            PodcastIndexCredentials(key: "env-key", secret: "env-secret")
        )
        XCTAssertEqual(
            PodcastCredentialStore.resolve(
                keychainKey: "stored-key",
                keychainSecret: "stored-secret",
                environment: ["PODCAST_INDEX_KEY": "env-key", "PODCAST_INDEX_SECRET": "env-secret"]
            ),
            PodcastIndexCredentials(key: "stored-key", secret: "stored-secret")
        )
    }

    func testRSSNamespacesPrioritiesAndChronologicalOrder() throws {
        let xml = """
        <?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:content="http://purl.org/rss/1.0/modules/content/">
          <channel>
            <title>Die Show</title><language>de</language><itunes:author>Show-Autorin</itunes:author>
            <itunes:owner><itunes:name>Verlag</itunes:name></itunes:owner>
            <description><![CDATA[<p>Show <b>Text</b></p>]]></description>
            <item><title>Älter</title><pubDate>Mon, 01 Sep 2025 08:00:00 +0000</pubDate><enclosure url="https://cdn.example/old.mp3" type="audio/mpeg"/></item>
            <item><title>Neu</title><dc:creator>Episode-Autor</dc:creator><pubDate>Mon, 15 Sep 2025 08:00:00 +0000</pubDate><itunes:episode>42</itunes:episode><itunes:season>3</itunes:season><itunes:duration>01:02:03</itunes:duration><content:encoded><![CDATA[<p>Episode &amp; Text</p>]]></content:encoded><guid>episode-42</guid><enclosure url="https://cdn.example/new.m4a" type="audio/mp4"/></item>
          </channel>
        </rss>
        """
        let feed = try PodcastRSSParser.parse(data: Data(xml.utf8), feedURL: URL(string: "https://example.org/feed.xml")!, podcastIndexFeedID: 123)
        XCTAssertEqual(feed.showTitle, "Die Show")
        XCTAssertEqual(feed.publisher, "Verlag")
        XCTAssertEqual(feed.episodes.map(\.title), ["Neu", "Älter"])
        XCTAssertEqual(feed.episodes[0].author, "Episode-Autor")
        XCTAssertEqual(feed.episodes[0].episodeNumber, 42)
        XCTAssertEqual(feed.episodes[0].seasonNumber, 3)
        XCTAssertEqual(feed.episodes[0].durationSeconds, 3723)
        XCTAssertEqual(feed.episodes[0].description, "Episode & Text")
    }

    func testMissingTitleOrEnclosureIsNotSelectable() throws {
        let xml = "<rss><channel><title>Show</title><item><guid>x</guid></item><item><title>Ohne Audio</title></item></channel></rss>"
        let feed = try PodcastRSSParser.parse(data: Data(xml.utf8), feedURL: URL(string: "https://example.org/feed")!)
        XCTAssertEqual(feed.episodes.count, 2)
        XCTAssertNotNil(feed.episodes[0].unavailableReason)
        XCTAssertNotNil(feed.episodes[1].unavailableReason)
    }

    @MainActor
    func testFilenameSanitizingMissingDateAndCollision() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let first = PodcastDownloadCoordinator.availableDestination(in: directory, publishedAt: nil, showTitle: "Show/Name", episodeTitle: "Titel: Eins")
        XCTAssertEqual(first.lastPathComponent, "Show–Name – Titel– Eins.mp3")
        XCTAssertTrue(FileManager.default.createFile(atPath: first.path, contents: Data()))
        let second = PodcastDownloadCoordinator.availableDestination(in: directory, publishedAt: nil, showTitle: "Show/Name", episodeTitle: "Titel: Eins")
        XCTAssertEqual(second.lastPathComponent, "Show–Name – Titel– Eins_2.mp3")
    }

    func testHTTPSRules() {
        XCTAssertTrue(SecureHTTPClient.isAllowedInitialURL(URL(string: "http://example.org/feed")!))
        XCTAssertTrue(SecureHTTPClient.isAllowedInitialURL(URL(string: "https://example.org/feed")!))
        XCTAssertFalse(SecureHTTPClient.isAllowedInitialURL(URL(string: "file:///tmp/feed")!))
        XCTAssertTrue(SecureHTTPClient.isAllowedRedirect(to: URL(string: "https://example.org/feed")!))
        XCTAssertFalse(SecureHTTPClient.isAllowedRedirect(to: URL(string: "http://example.org/feed")!))
    }

    func testOldTranscriptDocumentDecodesWithoutPodcast() throws {
        let json = """
        {"id":"1","source_path":"/tmp/a.mp3","source_file":"a.mp3","language":"de","model":"small","device":"metal","engine":"whisper.cpp","created":"2025-01-01","fps_timecode":25,"timecodes":true,"diarization":false,"speaker_names":{},"speaker_regions":[],"segments":[],"outputs":{}}
        """
        let document = try JSONDecoder().decode(TranscriptDocument.self, from: Data(json.utf8))
        XCTAssertNil(document.podcast)
    }

    @MainActor
    func testYouTubeAcceptsIndividualVideosAndRejectsPlaylists() throws {
        XCTAssertNoThrow(try YouTubeDownloadCoordinator.validatedURL("https://www.youtube.com/watch?v=abc123"))
        XCTAssertNoThrow(try YouTubeDownloadCoordinator.validatedURL("https://youtu.be/abc123"))
        XCTAssertNoThrow(try YouTubeDownloadCoordinator.validatedURL("https://www.youtube.com/shorts/abc123"))
        XCTAssertThrowsError(try YouTubeDownloadCoordinator.validatedURL("http://www.youtube.com/watch?v=abc123"))
        XCTAssertThrowsError(try YouTubeDownloadCoordinator.validatedURL("https://www.youtube.com/playlist?list=abc123"))
        XCTAssertThrowsError(try YouTubeDownloadCoordinator.validatedURL("https://example.org/watch?v=abc123"))
    }

    @MainActor
    func testYouTubeMetadataAndCollisionSafeDestinations() throws {
        let url = URL(string: "https://www.youtube.com/watch?v=abc123")!
        let payload = Data(#"{"id":"abc123","title":"Titel: Eins","channel":"Kanal","duration":65.4,"upload_date":"20260915","webpage_url":"https://www.youtube.com/watch?v=abc123"}"#.utf8)
        let info = try YouTubeDownloadCoordinator.decodeInfo(payload, fallbackURL: url)
        XCTAssertEqual(info.id, "abc123")
        XCTAssertEqual(info.title, "Titel: Eins")
        XCTAssertEqual(info.channel, "Kanal")
        XCTAssertEqual(info.durationSeconds, 65)

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let first = YouTubeDownloadCoordinator.availableDestinations(in: directory, info: info, videoExtension: "mp4")
        XCTAssertEqual(first.audio.lastPathComponent, "2026-09-15 – YouTube – Titel– Eins.mp3")
        XCTAssertEqual(first.video?.lastPathComponent, "2026-09-15 – YouTube – Titel– Eins.mp4")
        XCTAssertTrue(FileManager.default.createFile(atPath: first.audio.path, contents: Data()))
        let second = YouTubeDownloadCoordinator.availableDestinations(in: directory, info: info, videoExtension: "mp4")
        XCTAssertEqual(second.audio.lastPathComponent, "2026-09-15 – YouTube – Titel– Eins_2.mp3")
        XCTAssertEqual(second.video?.lastPathComponent, "2026-09-15 – YouTube – Titel– Eins_2.mp4")
    }
}
