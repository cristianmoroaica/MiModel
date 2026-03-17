use std::collections::HashSet;
use std::sync::LazyLock;
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedRef {
    pub name: String,
    pub source: DetectionSource,
    pub in_library: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DetectionSource {
    Marker,  // REF[...] from Claude
    Pattern, // regex match
}

static RE_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"REF\[([^\]]+)\]").expect("invalid RE_MARKER regex")
});

static RE_NEMA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bNEMA\s?\d{1,2}\b").expect("invalid RE_NEMA regex")
});

static RE_FASTENER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bM\d+x[\d.]+\b").expect("invalid RE_FASTENER regex")
});

static RE_BEARING: LazyLock<Regex> = LazyLock::new(|| {
    // Lookahead is not supported in the `regex` crate; we filter in post-processing
    Regex::new(r"\b\d{3,4}[A-Z]{2,3}\b").expect("invalid RE_BEARING regex")
});

pub fn detect_references(text: &str, known_slugs: &[String]) -> Vec<DetectedRef> {
    let mut seen_slugs: HashSet<String> = HashSet::new();
    let mut results: Vec<DetectedRef> = Vec::new();

    // 1. REF markers (highest priority — explicit)
    for cap in RE_MARKER.captures_iter(text) {
        let name = cap[1].trim().to_string();
        let slug = crate::reference::slug_from_name(&name);
        if seen_slugs.insert(slug.clone()) {
            let in_library = known_slugs.contains(&slug);
            results.push(DetectedRef {
                name,
                source: DetectionSource::Marker,
                in_library,
            });
        }
    }

    // 2. Known patterns
    // Collect spans occupied by REF[...] markers so pattern matches inside
    // marker brackets are excluded (e.g. NEMA 23 inside REF[NEMA 23 stepper]).
    let marker_spans: Vec<(usize, usize)> = RE_MARKER
        .find_iter(text)
        .map(|m| (m.start(), m.end()))
        .collect();

    let inside_marker = |start: usize, end: usize| -> bool {
        marker_spans.iter().any(|&(ms, me)| start >= ms && end <= me)
    };

    // For bearings: the `regex` crate does not support lookahead, so we emulate
    // the negative lookahead `(?![a-z])` by checking the char after the match.
    let bearing_matches = RE_BEARING.find_iter(text).filter(|m| {
        let end = m.end();
        !text[end..].starts_with(|c: char| c.is_ascii_lowercase())
    });

    let pattern_matches: Vec<&str> = RE_NEMA
        .find_iter(text)
        .chain(RE_FASTENER.find_iter(text))
        .chain(bearing_matches)
        .filter(|m| !inside_marker(m.start(), m.end()))
        .map(|m| m.as_str())
        .collect();

    for raw in pattern_matches {
        let name = raw.to_string();
        let slug = crate::reference::slug_from_name(&name);
        if seen_slugs.insert(slug.clone()) {
            let in_library = known_slugs.contains(&slug);
            results.push(DetectedRef {
                name,
                source: DetectionSource::Pattern,
                in_library,
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_ref_markers() {
        let text = "You should use REF[NEMA 23 stepper] for this axis.";
        let slugs: Vec<String> = vec![];
        let results = detect_references(text, &slugs);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "NEMA 23 stepper");
        assert_eq!(results[0].source, DetectionSource::Marker);
    }

    #[test]
    fn test_detect_known_patterns() {
        let text = "Use M3x8 screws and a NEMA17 motor for mounting.";
        let slugs: Vec<String> = vec![];
        let results = detect_references(text, &slugs);
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"M3x8"), "expected M3x8 in {:?}", names);
        assert!(names.contains(&"NEMA17"), "expected NEMA17 in {:?}", names);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_detect_in_library() {
        // slug_from_name("NEMA 17") == "nema_17"
        let text = "Mount this with a NEMA 17 stepper.";
        let slugs = vec!["nema_17".to_string()];
        let results = detect_references(text, &slugs);
        let found = results.iter().find(|r| r.name == "NEMA 17");
        assert!(found.is_some(), "NEMA 17 not detected in {:?}", results);
        assert!(found.unwrap().in_library, "expected in_library=true");
    }

    #[test]
    fn test_no_false_positives_on_plain_text() {
        let text = "The case is 38mm wide and 15mm tall.";
        let slugs: Vec<String> = vec![];
        let results = detect_references(text, &slugs);
        assert!(
            results.is_empty(),
            "expected no detections, got {:?}",
            results
        );
    }
}
