import Foundation

enum SpeakerEditingSupport {
    static func labels(in document: TranscriptDocument) -> [String] {
        var labels = Set(document.speakerNames.keys)
        labels.formUnion(document.speakerRegions.map(\.speaker))
        labels.formUnion(document.segments.compactMap(\.speaker))
        return labels.sorted()
    }

    static func merge(_ source: String, into target: String, in document: inout TranscriptDocument) {
        guard source != target else { return }
        let existingLabels = Set(labels(in: document))
        guard existingLabels.contains(source), existingLabels.contains(target) else { return }

        for index in document.segments.indices where document.segments[index].speaker == source {
            document.segments[index].speaker = target
        }
        for index in document.speakerRegions.indices where document.speakerRegions[index].speaker == source {
            document.speakerRegions[index].speaker = target
        }

        if document.speakerNames[target] == nil {
            document.speakerNames[target] = document.speakerNames[source] ?? target
        }
        document.speakerNames.removeValue(forKey: source)
        document.speakerRegions = coalesced(document.speakerRegions)
        document.diarization = !labels(in: document).isEmpty
    }

    private static func coalesced(_ regions: [SpeakerRegion]) -> [SpeakerRegion] {
        var result: [SpeakerRegion] = []
        for region in regions.sorted(by: { ($0.start, $0.end) < ($1.start, $1.end) }) {
            if let lastIndex = result.indices.last,
               result[lastIndex].speaker == region.speaker,
               region.start <= result[lastIndex].end + 0.5 {
                result[lastIndex].end = max(result[lastIndex].end, region.end)
            } else {
                result.append(region)
            }
        }
        return result
    }
}
