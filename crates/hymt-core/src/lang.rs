//! The languages Hy-MT2 was trained to translate between.
//!
//! The list mirrors the model card exactly (33 languages plus the dialect and ethnic
//! variants Tencent ships: Traditional Chinese, Cantonese, Tibetan, Kazakh, Mongolian and
//! Uyghur). Prompts must use the *full* language name, never the abbreviation, and the
//! Chinese template needs the Chinese name while the English template needs the English one
//! -- which is why both are stored next to each entry.

/// One selectable language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    /// Stable identifier used in settings and the UI (`ko`, `zh-Hant`, `yue`, ...).
    pub code: &'static str,
    /// Full English name, as required by HY-MT2's English prompt template.
    pub name_en: &'static str,
    /// Full Chinese name, as required by HY-MT2's Chinese prompt template.
    pub name_zh: &'static str,
    /// Name shown in the Korean UI.
    pub name_ko: &'static str,
}

impl Language {
    /// True for languages written in Chinese script. Used to decide whether the Chinese
    /// prompt template should be preferred when translating *into* this language.
    pub fn is_chinese(&self) -> bool {
        matches!(self.code, "zh" | "zh-Hant" | "yue")
    }

    /// Best-effort detection of the language of a text sample.
    ///
    /// This is intentionally conservative: it only recognizes scripts that can be told apart
    /// cheaply. Everything else returns `None`, and the UI keeps the current selection.
    pub fn detect(text: &str) -> Option<&'static Language> {
        let mut hangul = 0usize;
        let mut han = 0usize;
        let mut kana = 0usize;
        let mut cyrillic = 0usize;
        let mut latin = 0usize;

        for ch in text.chars() {
            match ch as u32 {
                0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3130..=0x318F => hangul += 1,
                0x3040..=0x30FF => kana += 1,
                0x4E00..=0x9FFF => han += 1,
                0x0400..=0x04FF => cyrillic += 1,
                _ if ch.is_ascii_alphabetic() => latin += 1,
                _ => {}
            }
        }

        // Japanese text always carries kana next to kanji; Korean text is dominated by
        // hangul syllables; Chinese text is han only.
        if kana > 0 {
            return by_code("ja");
        }
        if hangul > 0 && hangul >= han {
            return by_code("ko");
        }
        if han > 0 && han > hangul {
            return by_code("zh");
        }
        if cyrillic > 0 && cyrillic >= latin {
            // Cyrillic is shared; Ukrainian has distinctive letters.
            if text.chars().any(|c| matches!(c, 'і' | 'ї' | 'є' | 'ґ')) {
                return by_code("uk");
            }
            return by_code("ru");
        }
        None
    }
}

/// All languages supported by Hy-MT2, in model-card order.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "ko",
        name_en: "Korean",
        name_zh: "韩语",
        name_ko: "한국어",
    },
    Language {
        code: "en",
        name_en: "English",
        name_zh: "英语",
        name_ko: "영어",
    },
    Language {
        code: "ja",
        name_en: "Japanese",
        name_zh: "日语",
        name_ko: "일본어",
    },
    Language {
        code: "zh",
        name_en: "Chinese",
        name_zh: "中文",
        name_ko: "중국어(간체)",
    },
    Language {
        code: "zh-Hant",
        name_en: "Traditional Chinese",
        name_zh: "繁体中文",
        name_ko: "중국어(번체)",
    },
    Language {
        code: "yue",
        name_en: "Cantonese",
        name_zh: "粤语",
        name_ko: "광둥어",
    },
    Language {
        code: "de",
        name_en: "German",
        name_zh: "德语",
        name_ko: "독일어",
    },
    Language {
        code: "fr",
        name_en: "French",
        name_zh: "法语",
        name_ko: "프랑스어",
    },
    Language {
        code: "es",
        name_en: "Spanish",
        name_zh: "西班牙语",
        name_ko: "스페인어",
    },
    Language {
        code: "pt",
        name_en: "Portuguese",
        name_zh: "葡萄牙语",
        name_ko: "포르투갈어",
    },
    Language {
        code: "it",
        name_en: "Italian",
        name_zh: "意大利语",
        name_ko: "이탈리아어",
    },
    Language {
        code: "ru",
        name_en: "Russian",
        name_zh: "俄语",
        name_ko: "러시아어",
    },
    Language {
        code: "uk",
        name_en: "Ukrainian",
        name_zh: "乌克兰语",
        name_ko: "우크라이나어",
    },
    Language {
        code: "pl",
        name_en: "Polish",
        name_zh: "波兰语",
        name_ko: "폴란드어",
    },
    Language {
        code: "cs",
        name_en: "Czech",
        name_zh: "捷克语",
        name_ko: "체코어",
    },
    Language {
        code: "nl",
        name_en: "Dutch",
        name_zh: "荷兰语",
        name_ko: "네덜란드어",
    },
    Language {
        code: "tr",
        name_en: "Turkish",
        name_zh: "土耳其语",
        name_ko: "터키어",
    },
    Language {
        code: "ar",
        name_en: "Arabic",
        name_zh: "阿拉伯语",
        name_ko: "아랍어",
    },
    Language {
        code: "he",
        name_en: "Hebrew",
        name_zh: "希伯来语",
        name_ko: "히브리어",
    },
    Language {
        code: "fa",
        name_en: "Persian",
        name_zh: "波斯语",
        name_ko: "페르시아어",
    },
    Language {
        code: "hi",
        name_en: "Hindi",
        name_zh: "印地语",
        name_ko: "힌디어",
    },
    Language {
        code: "bn",
        name_en: "Bengali",
        name_zh: "孟加拉语",
        name_ko: "벵골어",
    },
    Language {
        code: "ta",
        name_en: "Tamil",
        name_zh: "泰米尔语",
        name_ko: "타밀어",
    },
    Language {
        code: "te",
        name_en: "Telugu",
        name_zh: "泰卢固语",
        name_ko: "텔루구어",
    },
    Language {
        code: "mr",
        name_en: "Marathi",
        name_zh: "马拉地语",
        name_ko: "마라티어",
    },
    Language {
        code: "gu",
        name_en: "Gujarati",
        name_zh: "古吉拉特语",
        name_ko: "구자라트어",
    },
    Language {
        code: "ur",
        name_en: "Urdu",
        name_zh: "乌尔都语",
        name_ko: "우르두어",
    },
    Language {
        code: "th",
        name_en: "Thai",
        name_zh: "泰语",
        name_ko: "태국어",
    },
    Language {
        code: "vi",
        name_en: "Vietnamese",
        name_zh: "越南语",
        name_ko: "베트남어",
    },
    Language {
        code: "id",
        name_en: "Indonesian",
        name_zh: "印尼语",
        name_ko: "인도네시아어",
    },
    Language {
        code: "ms",
        name_en: "Malay",
        name_zh: "马来语",
        name_ko: "말레이어",
    },
    Language {
        code: "tl",
        name_en: "Filipino",
        name_zh: "菲律宾语",
        name_ko: "필리핀어",
    },
    Language {
        code: "km",
        name_en: "Khmer",
        name_zh: "高棉语",
        name_ko: "크메르어",
    },
    Language {
        code: "my",
        name_en: "Burmese",
        name_zh: "缅甸语",
        name_ko: "미얀마어",
    },
    Language {
        code: "bo",
        name_en: "Tibetan",
        name_zh: "藏语",
        name_ko: "티베트어",
    },
    Language {
        code: "kk",
        name_en: "Kazakh",
        name_zh: "哈萨克语",
        name_ko: "카자흐어",
    },
    Language {
        code: "mn",
        name_en: "Mongolian",
        name_zh: "蒙古语",
        name_ko: "몽골어",
    },
    Language {
        code: "ug",
        name_en: "Uyghur",
        name_zh: "维吾尔语",
        name_ko: "위구르어",
    },
];

/// Looks a language up by its stable code.
pub fn by_code(code: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|lang| lang.code == code)
}

/// The language used for the source side on a fresh install.
pub fn default_source() -> &'static Language {
    by_code("ko").expect("Korean is part of the table")
}

/// The language used for the target side on a fresh install.
pub fn default_target() -> &'static Language {
    by_code("en").expect("English is part of the table")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn has_no_duplicate_codes_or_names() {
        let mut codes = HashSet::new();
        let mut names = HashSet::new();
        for lang in LANGUAGES {
            assert!(codes.insert(lang.code), "duplicate code {}", lang.code);
            assert!(
                names.insert(lang.name_en),
                "duplicate name {}",
                lang.name_en
            );
            assert!(!lang.name_ko.is_empty());
            assert!(!lang.name_zh.is_empty());
        }
    }

    #[test]
    fn covers_the_documented_model_card_languages() {
        assert_eq!(LANGUAGES.len(), 38);
        for code in [
            "zh", "en", "fr", "pt", "es", "ja", "tr", "ru", "ar", "ko", "th", "it", "de", "vi",
            "ms", "id", "tl", "hi", "zh-Hant", "pl", "cs", "nl", "km", "my", "fa", "gu", "ur",
            "te", "mr", "he", "bn", "ta", "uk", "bo", "kk", "mn", "ug", "yue",
        ] {
            assert!(by_code(code).is_some(), "missing {}", code);
        }
    }

    #[test]
    fn marks_only_chinese_script_languages_as_chinese() {
        assert!(by_code("zh").unwrap().is_chinese());
        assert!(by_code("zh-Hant").unwrap().is_chinese());
        assert!(by_code("yue").unwrap().is_chinese());
        assert!(!by_code("ja").unwrap().is_chinese());
        assert!(!by_code("ko").unwrap().is_chinese());
    }

    #[test]
    fn detects_common_scripts() {
        assert_eq!(Language::detect("안녕하세요").map(|l| l.code), Some("ko"));
        assert_eq!(Language::detect("こんにちは").map(|l| l.code), Some("ja"));
        assert_eq!(Language::detect("今天天气真好").map(|l| l.code), Some("zh"));
        assert_eq!(Language::detect("Привет").map(|l| l.code), Some("ru"));
        assert_eq!(Language::detect("Hello there").map(|l| l.code), None);
        assert_eq!(Language::detect("12345").map(|l| l.code), None);
    }
}
