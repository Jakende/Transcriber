from __future__ import annotations

import re
from collections import Counter
from collections.abc import Iterator


def extract_candidates(text: str, language: str, max_results: int = 100) -> list[dict]:
    if not text.strip():
        return []
    model_names = {
        "de": ["de_core_news_sm"],
        "en": ["en_core_web_sm"],
        "auto": ["de_core_news_sm", "en_core_web_sm"],
    }.get(language, ["de_core_news_sm", "en_core_web_sm"])
    try:
        import spacy

        extracted: list[list[dict]] = []
        for model_name in model_names:
            try:
                pipeline = spacy.load(model_name)
            except (OSError, ValueError):
                continue
            candidates: Counter[tuple[str, str]] = Counter()
            for document in pipeline.pipe(iter_text_chunks(text), batch_size=1):
                covered: set[int] = set()
                for entity in document.ents:
                    if entity.label_ in {"PER", "PERSON", "ORG", "LOC", "GPE", "MISC", "PRODUCT", "WORK_OF_ART"}:
                        term = entity.text.strip()
                        if len(term) >= 3:
                            candidates[(term, entity.label_)] += 1
                            covered.update(token.i for token in entity)
                for token in document:
                    if token.i not in covered and token.pos_ == "PROPN" and len(token.text) >= 3:
                        candidates[(token.text, "PROPN")] += 1
            extracted.append(merge_candidate_counts(candidates, max_results * 2))
        if not extracted:
            raise OSError("Kein passendes spaCy-Sprachmodell gefunden.")
        return merge_language_candidates(extracted, max_results)
    except (ImportError, OSError, ValueError):
        fallback = Counter(re.findall(r"(?<![.!?]\s)\b[A-ZÄÖÜ][\wÄÖÜäöüß'-]{2,}\b", text))
        return merge_candidate_counts(
            Counter({(term, "PROPN"): count for term, count in fallback.items()}),
            max_results,
        )


def merge_language_candidates(candidate_lists: list[list[dict]], max_results: int = 100) -> list[dict]:
    """Merge bilingual analyses without double-counting the same occurrence."""
    grouped: dict[str, dict] = {}
    for candidates in candidate_lists:
        for candidate in candidates:
            term = re.sub(r"\s+", " ", str(candidate.get("term", ""))).strip()
            count = max(0, int(candidate.get("count", 0)))
            if len(term) < 3 or count == 0:
                continue
            key = term.casefold()
            entry = grouped.setdefault(key, {"variants": Counter(), "kinds": set(), "count": 0})
            entry["variants"][term] += count
            entry["kinds"].update(
                kind.strip() for kind in str(candidate.get("kind", "PROPN")).split("/") if kind.strip()
            )
            entry["count"] = max(entry["count"], count)

    result = [
        {
            "term": entry["variants"].most_common(1)[0][0],
            "count": entry["count"],
            "kind": " / ".join(sorted(entry["kinds"])) or "PROPN",
        }
        for entry in grouped.values()
    ]
    result.sort(key=lambda item: (-item["count"], item["term"].casefold(), item["term"]))
    return result[:max_results]


def iter_text_chunks(text: str, max_characters: int = 100_000) -> Iterator[str]:
    """Yield sentence-aligned chunks below spaCy's default document limit."""
    remaining = text.strip()
    while remaining:
        if len(remaining) <= max_characters:
            yield remaining
            return
        window = remaining[:max_characters]
        boundaries = [window.rfind(marker) + len(marker) for marker in (". ", "? ", "! ", "\n")]
        cut = max(boundaries)
        if cut < max_characters // 2:
            cut = window.rfind(" ")
        if cut <= 0:
            cut = max_characters
        yield remaining[:cut].strip()
        remaining = remaining[cut:].strip()


def merge_candidate_counts(candidates: Counter[tuple[str, str]], max_results: int = 100) -> list[dict]:
    """Merge equal terms across entity kinds before they reach the Swift UI."""
    grouped: dict[str, dict] = {}
    for (raw_term, raw_kind), count in candidates.items():
        term = re.sub(r"\s+", " ", raw_term).strip()
        if len(term) < 3 or count <= 0:
            continue
        key = term.casefold()
        entry = grouped.setdefault(key, {"variants": Counter(), "kinds": Counter(), "count": 0})
        entry["variants"][term] += count
        entry["kinds"][raw_kind or "PROPN"] += count
        entry["count"] += count

    result = []
    for entry in grouped.values():
        term = entry["variants"].most_common(1)[0][0]
        kinds = sorted(entry["kinds"], key=lambda kind: (-entry["kinds"][kind], kind))
        result.append({"term": term, "count": entry["count"], "kind": " / ".join(kinds)})
    result.sort(key=lambda item: (-item["count"], item["term"].casefold(), item["term"]))
    return result[:max_results]
