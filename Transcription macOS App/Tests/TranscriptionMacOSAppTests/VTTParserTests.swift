import Foundation
import XCTest
@testable import TranscriptionMacOSApp

final class VTTParserTests: XCTestCase {
    func testImportsSpeakerLabelsAndTimes() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString).appendingPathExtension("vtt")
        try """
        WEBVTT

        1
        00:00:01.000 --> 00:00:03.500
        Anna: Guten Morgen.

        2
        00:00:04.000 --> 00:00:06.000
        Ben: Hallo Anna.
        """.write(to: url, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: url) }

        let document = try VTTParser.parse(vttURL: url, audioURL: nil)
        XCTAssertEqual(document.segments.count, 2)
        XCTAssertEqual(document.segments[0].start, 1, accuracy: 0.001)
        XCTAssertEqual(document.segments[0].end, 3.5, accuracy: 0.001)
        XCTAssertEqual(document.segments[0].text, "Guten Morgen.")
        XCTAssertEqual(Set(document.speakerNames.values), Set(["Anna", "Ben"]))
        XCTAssertTrue(document.sourceFile.contains("_bearbeitet"))
    }

    func testDocumentJSONRoundTrip() throws {
        let document = TranscriptDocument(
            id: "test", sourcePath: "/tmp/audio.wav", sourceFile: "audio.wav",
            language: "de", model: "small", device: "metal", engine: "whisper.cpp",
            created: "2026-08-02 12:00", fpsTimecode: 25, timecodes: true,
            diarization: false, speakerModel: nil, speakerNames: [:], speakerRegions: [],
            segments: [TranscriptSegment(id: "s1", start: 0, end: 1, speaker: nil, text: "Hallo")],
            outputs: ["markdown": "/tmp/audio.md"]
        )
        let data = try JSONEncoder().encode(document)
        let decoded = try JSONDecoder().decode(TranscriptDocument.self, from: data)
        XCTAssertEqual(decoded.id, document.id)
        XCTAssertEqual(decoded.segments.first?.text, "Hallo")
        XCTAssertEqual(decoded.outputs["markdown"], "/tmp/audio.md")
    }
}
