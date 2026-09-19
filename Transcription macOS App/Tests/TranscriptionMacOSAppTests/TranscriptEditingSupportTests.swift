import XCTest
@testable import TranscriptionMacOSApp

final class TranscriptEditingSupportTests: XCTestCase {
    func testSplitAndMergePreserveTextAndTimeRange() {
        var document = makeDocument()
        TranscriptEditingSupport.split(segmentID: "one", in: &document)
        XCTAssertEqual(document.segments.count, 2)
        XCTAssertEqual(document.segments.map(\.text), ["Hallo schöne", "neue Welt"])
        XCTAssertEqual(document.segments.first?.start, 0)
        XCTAssertEqual(document.segments.last?.end, 4)

        TranscriptEditingSupport.mergeWithNext(segmentID: "one", in: &document)
        XCTAssertEqual(document.segments.count, 1)
        XCTAssertEqual(document.segments[0].text, "Hallo schöne neue Welt")
        XCTAssertEqual(document.segments[0].end, 4)
    }

    func testGlossaryPromptContainsConfirmedReplacement() {
        let unique = "Fachbegriff-\(UUID().uuidString)"
        GlossaryStore.add(replacements: ["falsch": unique])
        XCTAssertTrue(GlossaryStore.prompt()?.contains(unique) == true)
    }

    private func makeDocument() -> TranscriptDocument {
        TranscriptDocument(
            id: "test", sourcePath: "/tmp/test.wav", sourceFile: "test.wav",
            language: "de", model: "small", device: "metal", engine: "whisper.cpp",
            created: "", fpsTimecode: 25, timecodes: true, diarization: false,
            speakerModel: nil, speakerNames: [:], speakerRegions: [],
            segments: [TranscriptSegment(id: "one", start: 0, end: 4, speaker: nil, text: "Hallo schöne neue Welt")],
            outputs: [:]
        )
    }
}
