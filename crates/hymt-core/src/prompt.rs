//! Prompt construction for Tencent's Hy-MT2 translation instructions.
//!
//! The templates below are transcribed from the official model card
//! (`tencent/Hy-MT2-1.8B-GGUF/README.md`, section "Hy-MT2 Translation Task Instruction
//! Examples"). Tencent's rule: *both source_lang and target_lang should use the full language
//! names; Chinese names are used in Chinese prompts and English names in English prompts.*
//!
//! The models have no default system prompt, so the instruction is the whole prompt.

use crate::lang::Language;

/// Which language the instruction itself should be written in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptLanguage {
    /// Chinese for Chinese-script targets, English otherwise (the model card's layout:
    /// ZH<=>XX uses the Chinese template, XX<=>XX the English one).
    #[default]
    Auto,
    /// Always use the English instruction template.
    English,
    /// Always use the Chinese instruction template.
    Chinese,
}

/// The translation task variant to encode into the prompt.
#[derive(Clone, Copy, Debug)]
pub enum PromptMode<'a> {
    /// Plain translation of `source` into the target language.
    Plain,
    /// Translation constrained to a style, e.g. `격식체` or `technical documentation`.
    Style {
        /// Free-form style description understood by the model.
        style: &'a str,
    },
    /// Translation with a terminology list applied (source term -> target term).
    Terminology {
        /// Glossary pairs, already trimmed and de-duplicated.
        pairs: &'a [(String, String)],
    },
    /// Translation that must preserve delimiters and formatting symbols one for one.
    Delimiters,
    /// Translation guided by extra background information.
    Context {
        /// Background text that explains the domain.
        context: &'a str,
    },
}

impl PromptLanguage {
    /// Resolves `Auto` into the concrete template language for a target language.
    pub fn resolve(self, target: &Language) -> PromptLanguage {
        match self {
            PromptLanguage::Auto if target.is_chinese() => PromptLanguage::Chinese,
            PromptLanguage::Auto => PromptLanguage::English,
            other => other,
        }
    }
}

/// Builds the instruction text sent to the model.
///
/// `source` is inserted verbatim; callers validate that it is not empty.
pub fn build_prompt(
    mode: PromptMode<'_>,
    source: &str,
    target: &Language,
    language: PromptLanguage,
) -> String {
    match language.resolve(target) {
        PromptLanguage::Chinese => build_chinese(mode, source, target),
        _ => build_english(mode, source, target),
    }
}

fn build_english(mode: PromptMode<'_>, source: &str, target: &Language) -> String {
    let target_name = target.name_en;
    match mode {
        PromptMode::Plain => format!(
            "Translate the following text into {target_name}. Note that you should only output the translated result without any additional explanation:\n\n{source}"
        ),
        PromptMode::Style { style } => format!(
            "Please translate the following text into {target_name}. Note that the translation style must strictly conform to [{style}]:\n\n{source}"
        ),
        PromptMode::Terminology { pairs } => {
            let mut out = String::from("Reference the following translations:\n");
            for (src, dst) in pairs {
                out.push_str(src);
                out.push_str(" translates to ");
                out.push_str(dst);
                out.push('\n');
            }
            out.push_str(&format!(
                "\nTranslate the following text into {target_name}. Note that you must ONLY output the translated result without any additional explanation:\n\n{source}"
            ));
            out
        }
        PromptMode::Delimiters => format!(
            "Please accurately translate the following text into {target_name}. You must retain the exact same number of delimiters in the translation. Strictly do not omit, escape, or translate these symbols, and pay close attention to their placement:\n\n{source}"
        ),
        PromptMode::Context { context } => format!(
            "[Background Information]\n{context}\n\nPlease translate the following text into {target_name}, taking the provided background information into consideration.\n\n[Source Text]\n{source}"
        ),
    }
}

fn build_chinese(mode: PromptMode<'_>, source: &str, target: &Language) -> String {
    let target_name = target.name_zh;
    match mode {
        PromptMode::Plain => format!(
            "将以下文本翻译为{target_name}，注意只需要输出翻译后的结果，不要额外解释：\n\n{source}"
        ),
        PromptMode::Style { style } => format!(
            "请将以下文本翻译为{target_name}。\n注意翻译的风格要严格符合【{style}】\n\n{source}"
        ),
        PromptMode::Terminology { pairs } => {
            let mut out = String::from("参考下面的翻译：\n");
            for (src, dst) in pairs {
                out.push_str(src);
                out.push_str(" 翻译成 ");
                out.push_str(dst);
                out.push('\n');
            }
            out.push_str(&format!(
                "将以下文本翻译为{target_name}，注意只需要输出翻译后的结果，不要额外解释：\n\n{source}"
            ));
            out
        }
        PromptMode::Delimiters => format!(
            "请将以下文本准确翻译为{target_name}。\n你必须在译文中保留等量的分隔符，绝对不可遗漏、转义或翻译该符号，并注意分隔符的位置。\n\n{source}"
        ),
        PromptMode::Context { context } => format!(
            "【背景信息】\n{context}\n\n请结合背景信息将以下文本翻译为{target_name}。\n\n【待翻译文本】\n{source}"
        ),
    }
}

/// Returns an error message when the source side cannot be translated.
pub fn validate_source(source: &str) -> Result<(), &'static str> {
    if source.trim().is_empty() {
        Err("번역할 텍스트를 입력하세요.")
    } else if source.chars().count() > 20_000 {
        Err("텍스트가 너무 깁니다. 20,000자 이하로 나눠서 번역하세요.")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::by_code;

    fn ko() -> &'static Language {
        by_code("ko").unwrap()
    }
    fn zh() -> &'static Language {
        by_code("zh").unwrap()
    }
    fn en() -> &'static Language {
        by_code("en").unwrap()
    }

    #[test]
    fn plain_english_prompt_matches_model_card() {
        let prompt = build_prompt(PromptMode::Plain, "안녕하세요", en(), PromptLanguage::Auto);
        assert_eq!(
            prompt,
            "Translate the following text into English. Note that you should only output the translated result without any additional explanation:\n\n안녕하세요"
        );
    }

    #[test]
    fn plain_chinese_prompt_is_used_for_chinese_targets() {
        let prompt = build_prompt(
            PromptMode::Plain,
            "It’s on the house.",
            zh(),
            PromptLanguage::Auto,
        );
        assert_eq!(
            prompt,
            "将以下文本翻译为中文，注意只需要输出翻译后的结果，不要额外解释：\n\nIt’s on the house."
        );
    }

    #[test]
    fn explicit_prompt_language_overrides_the_auto_rule() {
        let english = build_prompt(PromptMode::Plain, "안녕", zh(), PromptLanguage::English);
        assert!(english.starts_with("Translate the following text into Chinese."));
        let chinese = build_prompt(PromptMode::Plain, "hi", en(), PromptLanguage::Chinese);
        assert!(chinese.starts_with("将以下文本翻译为英语"));
    }

    #[test]
    fn terminology_list_precedes_the_instruction() {
        let pairs = vec![
            ("GPUI".to_string(), "지피유아이".to_string()),
            ("Zed".to_string(), "제드".to_string()),
        ];
        let prompt = build_prompt(
            PromptMode::Terminology { pairs: &pairs },
            "GPUI powers Zed.",
            ko(),
            PromptLanguage::English,
        );
        assert!(prompt.starts_with(
            "Reference the following translations:\nGPUI translates to 지피유아이\nZed translates to 제드\n"
        ));
        assert!(prompt.contains("Translate the following text into Korean."));
        assert!(prompt.ends_with("GPUI powers Zed."));
    }

    #[test]
    fn style_and_delimiters_and_context_templates() {
        let style = build_prompt(
            PromptMode::Style { style: "격식체" },
            "hi",
            ko(),
            PromptLanguage::English,
        );
        assert!(style.contains("must strictly conform to [격식체]"));

        let delimiters = build_prompt(PromptMode::Delimiters, "hi", ko(), PromptLanguage::English);
        assert!(delimiters.contains("exact same number of delimiters"));

        let context = build_prompt(
            PromptMode::Context {
                context: "의료 문서",
            },
            "hi",
            zh(),
            PromptLanguage::Auto,
        );
        assert!(context.starts_with("【背景信息】\n의료 문서"));
        assert!(context.contains("【待翻译文本】\nhi"));
    }

    #[test]
    fn chinese_terminology_template() {
        let pairs = vec![("Transformer".to_string(), "变换器".to_string())];
        let prompt = build_prompt(
            PromptMode::Terminology { pairs: &pairs },
            "text",
            zh(),
            PromptLanguage::Auto,
        );
        assert!(prompt.starts_with("参考下面的翻译：\nTransformer 翻译成 变换器\n"));
        assert!(prompt.contains("将以下文本翻译为中文"));
    }

    #[test]
    fn source_validation_rejects_empty_and_huge_input() {
        assert!(validate_source("   ").is_err());
        assert!(validate_source("hi").is_ok());
        assert!(validate_source(&"가".repeat(20_001)).is_err());
    }
}
