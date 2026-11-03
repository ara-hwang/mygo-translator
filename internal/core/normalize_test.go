package core

import (
	"fmt"
	"testing"
)

func TestNormalizeTrimsAndNormalizesNewlines(t *testing.T) {
	if got := NormalizeOutput("  안녕하세요\r\n"); got != "안녕하세요" {
		t.Fatalf("got %q", got)
	}
	if got := NormalizeOutput("a\r\nb"); got != "a\nb" {
		t.Fatalf("got %q", got)
	}
}

func TestStripHunyuanControlTokens(t *testing.T) {
	if got := NormalizeOutput("안녕하세요<｜hy_place▁holder▁no▁2｜>"); got != "안녕하세요" {
		t.Fatalf("got %q", got)
	}
	if got := StripSpecialTokens("<|startoftext|>hello<|extra_0|>"); got != "hello" {
		t.Fatalf("got %q", got)
	}
}

func TestKeepsRealMarkup(t *testing.T) {
	text := "<div>번역</div> <b>굵게</b> 1 < 2"
	if got := StripSpecialTokens(text); got != text {
		t.Fatalf("got %q want %q", got, text)
	}
}

func TestStripSpecialTokensWithoutSlicingInsideUtf8(t *testing.T) {
	text := fmt.Sprintf("<%s>", repeatRune('한', 40))
	if got := StripSpecialTokens(text); got != text {
		t.Fatalf("got %q want %q", got, text)
	}
}

func repeatRune(r rune, n int) string {
	out := make([]rune, n)
	for i := range out {
		out[i] = r
	}
	return string(out)
}
