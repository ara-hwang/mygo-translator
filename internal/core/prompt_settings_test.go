package core

import (
	"testing"
)

func TestBuildPromptSettings(t *testing.T) {
	const source = "  API\n안녕하세요  "
	for _, tc := range []struct {
		name     string
		settings PromptSettings
		target   string
		want     string
	}{
		{"plain preserves source", PromptSettings{}, "en", "Translate the following text into English. Note that you should only output the translated result without any additional explanation:\n\n" + source},
		{"unknown target falls back to English", PromptSettings{}, "invalid", "Translate the following text into English. Note that you should only output the translated result without any additional explanation:\n\n" + source},
		{"Chinese auto", PromptSettings{}, "zh", "将以下文本翻译为中文，注意只需要输出翻译后的结果，不要额外解释：\n\n" + source},
		{"forced English", PromptSettings{Language: PromptEnglish}, "zh", "Translate the following text into Chinese. Note that you should only output the translated result without any additional explanation:\n\n" + source},
		{"forced Chinese", PromptSettings{Language: PromptChinese}, "en", "将以下文本翻译为英语，注意只需要输出翻译后的结果，不要额外解释：\n\n" + source},
		{"style", PromptSettings{Style: "격식체"}, "en", "Please translate the following text into English. Note that the translation style must strictly conform to [격식체]:\n\n" + source},
		{"disabled glossary keeps style", PromptSettings{Style: "격식체", Glossary: "API=>인터페이스"}, "en", "Please translate the following text into English. Note that the translation style must strictly conform to [격식체]:\n\n" + source},
		{"glossary precedes style", PromptSettings{Style: "격식체", UseGlossary: true, Glossary: "API=>인터페이스"}, "en", "Reference the following translations:\nAPI translates to 인터페이스\n\nTranslate the following text into English. Note that you must ONLY output the translated result without any additional explanation:\n\n" + source},
		{"empty glossary falls back to style", PromptSettings{Style: "격식체", UseGlossary: true, Glossary: "invalid"}, "en", "Please translate the following text into English. Note that the translation style must strictly conform to [격식체]:\n\n" + source},
		{"delimiters precede glossary and style", PromptSettings{Style: "격식체", UseGlossary: true, Glossary: "API=>인터페이스", PreserveDelimiters: true}, "en", "Please accurately translate the following text into English. You must retain the exact same number of delimiters in the translation. Strictly do not omit, escape, or translate these symbols, and pay close attention to their placement:\n\n" + source},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if got := BuildPromptFromSettings(tc.settings, source, tc.target); got != tc.want {
				t.Fatalf("prompt = %q, want %q", got, tc.want)
			}
		})
	}
}
