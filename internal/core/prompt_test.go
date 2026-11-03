package core

import (
	"strings"
	"testing"
)

func ko() Language { l, _ := ByCode("ko"); return l }
func zh() Language { l, _ := ByCode("zh"); return l }
func en() Language { l, _ := ByCode("en"); return l }

func TestPlainEnglishPromptMatchesModelCard(t *testing.T) {
	p := BuildPrompt(PromptMode{}, "안녕하세요", en(), PromptAuto)
	want := "Translate the following text into English. Note that you should only output the translated result without any additional explanation:\n\n안녕하세요"
	if p != want {
		t.Fatalf("gold mismatch:\n%q\nwant:\n%q", p, want)
	}
}

func TestPlainChinesePromptIsUsedForChineseTargets(t *testing.T) {
	p := BuildPrompt(PromptMode{}, "It’s on the house.", zh(), PromptAuto)
	want := "将以下文本翻译为中文，注意只需要输出翻译后的结果，不要额外解释：\n\nIt’s on the house."
	if p != want {
		t.Fatalf("gold mismatch:\n%q\nwant:\n%q", p, want)
	}
}

func TestExplicitPromptLanguageOverridesAutoRule(t *testing.T) {
	english := BuildPrompt(PromptMode{}, "안녕", zh(), PromptEnglish)
	if !strings.HasPrefix(english, "Translate the following text into Chinese.") {
		t.Fatalf("want English template, got %q", english)
	}
	chinese := BuildPrompt(PromptMode{}, "hi", en(), PromptChinese)
	if !strings.HasPrefix(chinese, "将以下文本翻译为英语") {
		t.Fatalf("want Chinese template, got %q", chinese)
	}
}

func TestTerminologyListPrecedesInstruction(t *testing.T) {
	pairs := [][2]string{{"GPUI", "지피유아이"}, {"Zed", "제드"}}
	p := BuildPrompt(PromptMode{Pairs: pairs}, "GPUI powers Zed.", ko(), PromptEnglish)
	if !strings.HasPrefix(p, "Reference the following translations:\nGPUI translates to 지피유아이\nZed translates to 제드\n") {
		t.Fatalf("terminology prefix mismatch: %q", p)
	}
	if !strings.Contains(p, "Translate the following text into Korean.") {
		t.Fatalf("missing instruction: %q", p)
	}
	if !strings.HasSuffix(p, "GPUI powers Zed.") {
		t.Fatalf("source must be last, got %q", p)
	}
}

func TestStyleAndDelimitersAndContextTemplates(t *testing.T) {
	style := BuildPrompt(PromptMode{Style: "격식체"}, "hi", ko(), PromptEnglish)
	if !strings.Contains(style, "must strictly conform to [격식체]") {
		t.Fatalf("style template mismatch: %q", style)
	}
	delimiters := BuildPrompt(PromptMode{Delimiters: true}, "hi", ko(), PromptEnglish)
	if !strings.Contains(delimiters, "exact same number of delimiters") {
		t.Fatalf("delimiters template mismatch: %q", delimiters)
	}
	ctx := BuildPrompt(PromptMode{Context: "의료 문서"}, "hi", zh(), PromptAuto)
	if !strings.HasPrefix(ctx, "【背景信息】\n의료 문서") || !strings.Contains(ctx, "【待翻译文本】\nhi") {
		t.Fatalf("context template mismatch: %q", ctx)
	}
}

func TestChineseTerminologyTemplate(t *testing.T) {
	p := BuildPrompt(PromptMode{Pairs: [][2]string{{"Transformer", "变换器"}}}, "text", zh(), PromptAuto)
	if !strings.HasPrefix(p, "参考下面的翻译：\nTransformer 翻译成 变换器\n") {
		t.Fatalf("chinese terminology mismatch: %q", p)
	}
	if !strings.Contains(p, "将以下文本翻译为中文") {
		t.Fatalf("chinese instruction mismatch: %q", p)
	}
}

func TestSourceValidationRejectsEmptyAndHugeInput(t *testing.T) {
	if ok, _ := ValidateSource("   "); ok {
		t.Fatal("blank source must fail")
	}
	if ok, _ := ValidateSource("hi"); !ok {
		t.Fatal("normal source must pass")
	}
	if ok, _ := ValidateSource(strings.Repeat("가", 20001)); ok {
		t.Fatal("oversized source must fail")
	}
}
