import Foundation
import XCTest
import UniformTypeIdentifiers
@testable import TranscriptionMacOSApp

final class MediaFileSupportTests: XCTestCase {
    func testCommonVideoContainersAreAcceptedCaseInsensitively() {
        for name in ["film.mp4", "film.MOV", "film.mkv", "film.webm", "film.avi", "film.m2ts", "film.wmv"] {
            XCTAssertTrue(MediaFileSupport.isSupported(URL(fileURLWithPath: "/tmp/\(name)")), name)
        }
    }

    func testUnsupportedFilesAndDirectoriesAreRejected() {
        XCTAssertFalse(MediaFileSupport.isSupported(URL(fileURLWithPath: "/tmp/notiz.pdf")))
        XCTAssertFalse(MediaFileSupport.isSupported(URL(fileURLWithPath: "/tmp/ordner", isDirectory: true)))
    }

    func testPanelTypesContainExtendedVideoTypes() {
        let identifiers = Set(MediaFileSupport.openPanelContentTypes.map(\.identifier))
        XCTAssertFalse(identifiers.isEmpty)
        XCTAssertNotNil(UTType(filenameExtension: "mkv"))
        XCTAssertNotNil(UTType(filenameExtension: "webm"))
    }
}
