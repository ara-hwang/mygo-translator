//! Glossary parsing for Hy-MT2's terminology-intervention prompt.
//!
//! A glossary line maps one source term to one target term:
//!
//! ```text
//! # 테크 용어
//! GPUI => 지피유아이
//! Zed	제드
//! ```
//!
//! `=>` is the canonical separator; a single tab also works so a two-column spreadsheet
//! can be pasted directly.

/// Maximum number of terminology pairs forwarded to the model.
pub const MAX_ENTRIES: usize = 200;
/// Maximum length of a single term, in characters.
pub const MAX_TERM_CHARS: usize = 120;

/// Parses glossary text, dropping comments, blank lines and malformed entries.
pub fn parse(text: &str) -> Vec<(String, String)> {
    parse_report(text).0
}

/// Parses glossary text and reports why entries were skipped.
pub fn parse_report(text: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut warnings = Vec::new();

    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let Some((source, target)) = split_pair(line) else {
            warnings.push(format!(
                "{}번째 줄: 구분자(`=>` 또는 탭)가 없습니다.",
                index + 1
            ));
            continue;
        };
        let source = source.trim();
        let target = target.trim();
        if source.is_empty() || target.is_empty() {
            warnings.push(format!(
                "{}번째 줄: 원문 또는 번역이 비어 있습니다.",
                index + 1
            ));
            continue;
        }
        if source.chars().count() > MAX_TERM_CHARS || target.chars().count() > MAX_TERM_CHARS {
            warnings.push(format!(
                "{}번째 줄: 용어가 너무 깁니다(최대 {}자).",
                index + 1,
                MAX_TERM_CHARS
            ));
            continue;
        }
        if pairs.len() >= MAX_ENTRIES {
            warnings.push(format!("용어집은 최대 {}개까지 사용합니다.", MAX_ENTRIES));
            break;
        }
        // Later definitions win; keeping the list free of duplicates keeps the prompt short.
        if let Some(existing) = pairs.iter_mut().find(|(src, _)| src == source) {
            existing.1 = target.to_string();
        } else {
            pairs.push((source.to_string(), target.to_string()));
        }
    }

    (pairs, warnings)
}

fn split_pair(line: &str) -> Option<(&str, &str)> {
    if let Some((left, right)) = line.split_once("=>") {
        return Some((left, right));
    }
    line.split_once('\t')
}

/// Renders pairs back into the canonical `source=>target` form.
pub fn format(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(src, dst)| format!("{src} => {dst}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arrow_and_tab_separators() {
        let text = "GPUI => 지피유아이\nZed\t제드\n\n# comment\n";
        assert_eq!(
            parse(text),
            vec![
                ("GPUI".to_string(), "지피유아이".to_string()),
                ("Zed".to_string(), "제드".to_string())
            ]
        );
    }

    #[test]
    fn reports_malformed_lines() {
        let (pairs, warnings) = parse_report("no separator here\nen => 영어\n=>empty\n");
        assert_eq!(pairs, vec![("en".to_string(), "영어".to_string())]);
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].contains("1번째"));
    }

    #[test]
    fn later_duplicates_replace_earlier_ones() {
        let pairs = parse("API => 에이피아이\nAPI => 인터페이스");
        assert_eq!(pairs, vec![("API".to_string(), "인터페이스".to_string())]);
    }

    #[test]
    fn caps_the_number_of_entries() {
        let text = (0..MAX_ENTRIES + 5)
            .map(|i| format!("term{i} => 용어{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (pairs, warnings) = parse_report(&text);
        assert_eq!(pairs.len(), MAX_ENTRIES);
        assert!(warnings.last().unwrap().contains("최대"));
    }

    #[test]
    fn formats_and_reparses_identically() {
        let pairs = vec![("a".to_string(), "b".to_string())];
        assert_eq!(parse(&format(&pairs)), pairs);
    }
}
