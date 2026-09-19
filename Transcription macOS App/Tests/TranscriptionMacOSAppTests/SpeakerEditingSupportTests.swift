import XCTest
@testable import TranscriptionMacOSApp

final class SpeakerEditingSupportTests: XCTestCase {
    func testMergeReassignsSegmentsRegionsAndNames() {
        var document = TranscriptDocument(
            id: "test",
            sourcePath: "/tmp/test.wav",
            sourceFile: "test.wav",
            language: "auto",
            model: "turbo",
            device: "metal",
            engine: "whisper.cpp",
            created: "2026-08-04 10:00",
            fpsTimecode: 25,
            timecodes: true,
            diarization: true,
            speakerModel: "test",
            speakerNames: ["SPEAKER_00": "Anna", "SPEAKER_01": "Anna (2)"],
            speakerRegions: [
                SpeakerRegion(start: 0, end: 1, speaker: "SPEAKER_00"),
                SpeakerRegion(start: 1.2, end: 2, speaker: "SPEAKER_01"),
            ],
            segments: [
                TranscriptSegment(id: "1", start: 0, end: 1, speaker: "SPEAKER_00", text: "Hallo"),
                TranscriptSegment(id: "2", start: 1.2, end: 2, speaker: "SPEAKER_01", text: "Hello"),
            ],
            outputs: [:]
        )

        SpeakerEditingSupport.merge("SPEAKER_01", into: "SPEAKER_00", in: &document)

        XCTAssertEqual(Set(document.segments.compactMap(\.speaker)), ["SPEAKER_00"])
        XCTAssertEqual(document.speakerNames, ["SPEAKER_00": "Anna"])
        XCTAssertEqual(document.speakerRegions, [SpeakerRegion(start: 0, end: 2, speaker: "SPEAKER_00")])
    }

    func testNormalSeparationIsMoreConservativeThanStrict() {
        XCTAssertGreaterThan(SeparationPreset.normal.threshold, SeparationPreset.strict.threshold)
        XCTAssertEqual(TranscriptLanguage.mixed.rawValue, "auto")
    }
}
