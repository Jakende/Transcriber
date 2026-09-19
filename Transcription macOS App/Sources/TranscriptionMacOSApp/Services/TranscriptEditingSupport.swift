import Foundation

enum TranscriptEditingSupport {
    static func split(segmentID: String, in document: inout TranscriptDocument) {
        guard let index = document.segments.firstIndex(where: { $0.id == segmentID }) else { return }
        let segment = document.segments[index]
        let words = segment.text.split(separator: " ", omittingEmptySubsequences: true)
        guard words.count >= 2, segment.end > segment.start else { return }

        let wordIndex = max(1, words.count / 2)
        let firstText = words[..<wordIndex].joined(separator: " ")
        let secondText = words[wordIndex...].joined(separator: " ")
        let midpoint = segment.start + (segment.end - segment.start) * Double(wordIndex) / Double(words.count)
        document.segments[index].end = midpoint
        document.segments[index].text = firstText
        document.segments.insert(
            TranscriptSegment(
                id: UUID().uuidString,
                start: midpoint,
                end: segment.end,
                speaker: segment.speaker,
                text: secondText
            ),
            at: index + 1
        )
    }

    static func mergeWithNext(segmentID: String, in document: inout TranscriptDocument) {
        guard let index = document.segments.firstIndex(where: { $0.id == segmentID }),
              document.segments.indices.contains(index + 1) else { return }
        let next = document.segments[index + 1]
        document.segments[index].end = max(document.segments[index].end, next.end)
        document.segments[index].text = [document.segments[index].text, next.text]
            .filter { !$0.isEmpty }
            .joined(separator: " ")
        if document.segments[index].speaker == nil { document.segments[index].speaker = next.speaker }
        document.segments.remove(at: index + 1)
    }
}
