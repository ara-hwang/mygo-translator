package core

import "strings"

// PromptLanguage decides which language the instruction itself is written in.
type PromptLanguage int

const (
	// PromptAuto uses Chinese for Chinese-script targets, English otherwise
	// (the model card's layout: ZH<=>XX uses the Chinese template, XX<=>XX
	// the English one).
	PromptAuto PromptLanguage = iota
	// PromptEnglish always uses the English instruction template.
	PromptEnglish
	// PromptChinese always uses the Chinese instruction template.
	PromptChinese
)

// PromptMode describes the translation task variant to encode into the prompt.
type PromptMode struct {
	// Style, when non-empty, constrains the translation, e.g. "격식체".
	Style string
	// Pairs, when non-empty, applies a terminology list (source -> target).
	Pairs [][2]string
	// Delimiters asks for delimiter-preserving translation.
	Delimiters bool
	// Context, when non-empty, gives extra background information.
	Context string
}

// Resolve folds PromptAuto into the concrete template language for a target.
func (p PromptLanguage) Resolve(target Language) PromptLanguage {
	if p != PromptAuto {
		return p
	}
	if target.IsChinese() {
		return PromptChinese
	}
	return PromptEnglish
}

// BuildPrompt builds the instruction text sent to the model. `source` is
// inserted verbatim; callers validate that it is not empty.
func BuildPrompt(mode PromptMode, source string, target Language, language PromptLanguage) string {
	switch language.Resolve(target) {
	case PromptChinese:
		return buildChinesePrompt(mode, source, target)
	default:
		return buildEnglishPrompt(mode, source, target)
	}
}

func terminologyBlock(pairs [][2]string, joiner string) string {
	var b strings.Builder
	for _, pair := range pairs {
		b.WriteString(pair[0])
		b.WriteString(joiner)
		b.WriteString(pair[1])
		b.WriteByte('\n')
	}
	return b.String()
}

func buildEnglishPrompt(mode PromptMode, source string, target Language) string {
	name := target.NameEn
	switch {
	case len(mode.Pairs) > 0:
		out := "Reference the following translations:\n" +
			terminologyBlock(mode.Pairs, " translates to ") +
			"\nTranslate the following text into " + name +
			". Note that you must ONLY output the translated result without any additional explanation:\n\n" +
			source
		return out
	case mode.Context != "":
		return "[Background Information]\n" + mode.Context +
			"\n\nPlease translate the following text into " + name +
			", taking the provided background information into consideration.\n\n[Source Text]\n" + source
	case mode.Delimiters:
		return "Please accurately translate the following text into " + name +
			". You must retain the exact same number of delimiters in the translation. Strictly do not omit, escape, or translate these symbols, and pay close attention to their placement:\n\n" + source
	case mode.Style != "":
		return "Please translate the following text into " + name +
			". Note that the translation style must strictly conform to [" + mode.Style + "]:\n\n" + source
	default:
		return "Translate the following text into " + name +
			". Note that you should only output the translated result without any additional explanation:\n\n" + source
	}
}

func buildChinesePrompt(mode PromptMode, source string, target Language) string {
	name := target.NameZh
	switch {
	case len(mode.Pairs) > 0:
		out := "参考下面的翻译：\n" +
			terminologyBlock(mode.Pairs, " 翻译成 ") +
			"将以下文本翻译为" + name +
			"，注意只需要输出翻译后的结果，不要额外解释：\n\n" + source
		return out
	case mode.Context != "":
		return "【背景信息】\n" + mode.Context +
			"\n\n请结合背景信息将以下文本翻译为" + name +
			"。\n\n【待翻译文本】\n" + source
	case mode.Delimiters:
		return "请将以下文本准确翻译为" + name +
			"。\n你必须在译文中保留等量的分隔符，绝对不可遗漏、转义或翻译该符号，并注意分隔符的位置。\n\n" + source
	case mode.Style != "":
		return "请将以下文本翻译为" + name +
			"。\n注意翻译的风格要严格符合【" + mode.Style + "】\n\n" + source
	default:
		return "将以下文本翻译为" + name +
			"，注意只需要输出翻译后的结果，不要额外解释：\n\n" + source
	}
}

// ValidateSource returns false (with a user-facing message) when the source
// side cannot be translated.
func ValidateSource(source string) (bool, string) {
	if strings.TrimSpace(source) == "" {
		return false, "번역할 텍스트를 입력하세요."
	}
	if len([]rune(source)) > 20_000 {
		return false, "텍스트가 너무 깁니다. 20,000자 이하로 나눠서 번역하세요."
	}
	return true, ""
}
