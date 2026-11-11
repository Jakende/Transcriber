use std::collections::BTreeMap;
/// Apply whole-term edits; long phrases take precedence and replacement text stays literal.
pub fn apply(text: &str, replacements: &BTreeMap<String, String>) -> String {
    let mut selected = replacements
        .iter()
        .map(|(a, b)| (a.trim(), b.trim()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty() && a != b)
        .collect::<Vec<_>>();
    selected.sort_by_key(|(source, _)| std::cmp::Reverse(source.chars().count()));
    let mut result = text.to_string();
    for (source, target) in selected {
        let Ok(regex) = regex::RegexBuilder::new(&regex::escape(source))
            .case_insensitive(true)
            .build()
        else {
            continue;
        };
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut output = String::new();
        let mut last = 0;
        for found in regex.find_iter(&result) {
            let left = result[..found.start()]
                .chars()
                .next_back()
                .is_none_or(|c| !word(c));
            let right = result[found.end()..]
                .chars()
                .next()
                .is_none_or(|c| !word(c));
            if left && right {
                output.push_str(&result[last..found.start()]);
                output.push_str(target);
                last = found.end();
            }
        }
        output.push_str(&result[last..]);
        result = output;
    }
    result
}
pub fn language(text: &str) -> Option<String> {
    if text.trim().chars().count() < 12 {
        return None;
    }
    let detection = whatlang::detect(text)?;
    if detection.confidence() < 0.55 {
        return None;
    }
    match detection.lang() {
        whatlang::Lang::Deu => Some("de".into()),
        whatlang::Lang::Eng => Some("en".into()),
        _ => None,
    }
}
