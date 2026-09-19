import XCTest
@testable import TranscriptionMacOSApp

final class GlossarySupportTests: XCTestCase {
    func testDuplicateTermsAreMergedWithoutDictionaryTrap() {
        let candidates = [
            GlossaryCandidate(term: "Musk", count: 2, kind: "PER"),
            GlossaryCandidate(term: "Musk", count: 1, kind: "PROPN"),
            GlossaryCandidate(term: "musk", count: 3, kind: "ORG"),
        ]

        let merged = GlossarySupport.merged(candidates)
        XCTAssertEqual(merged.count, 1)
        XCTAssertEqual(merged[0].term, "Musk")
        XCTAssertEqual(merged[0].count, 6)
        XCTAssertEqual(Set(merged[0].kind.components(separatedBy: " / ")), Set(["ORG", "PER", "PROPN"]))
        XCTAssertEqual(GlossarySupport.replacementMap(for: candidates, preserving: [:]).count, 1)
    }

    func testReplacementValuesSurviveRepeatedAnalysis() {
        let previous = ["MUSK": "Elon Musk"]
        let map = GlossarySupport.replacementMap(
            for: [GlossaryCandidate(term: "Musk", count: 2, kind: "PER")],
            preserving: previous
        )
        XCTAssertEqual(map["Musk"], "Elon Musk")
    }

    func testReplacementUsesUnicodeWordBoundariesAndLiteralTarget() {
        let result = GlossarySupport.applying(
            ["Plan": "$Entwurf", "Musk": "Elon Musk"],
            to: "Plan, PLAN und Planer treffen Musk."
        )
        XCTAssertEqual(result, "$Entwurf, $Entwurf und Planer treffen Elon Musk.")
    }
}
