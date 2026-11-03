package core

import "strings"

// NormalizeOutput turns raw model output into the text shown to the user.
//
// Hy-MT2 is instructed to answer with the translation only. In practice a
// model can still leak its chat-template control tokens (for example
// `<｜hy_place▁holder▁no▁2｜>`) when it hits the generation limit, or wrap the
// answer in quotes. Those are removed here so the UI never shows plumbing.
func NormalizeOutput(raw string) string {
	text := strings.ReplaceAll(raw, "\r\n", "\n")
	text = StripSpecialTokens(text)
	return strings.TrimSpace(text)
}

// knownTokenPrefixes marks chat-template control tokens such as
// <|startoftext|> and <|extra_0|>.
var knownTokenPrefixes = []string{
	"startoftext", "endoftext", "eos", "bos", "extra_", "im_start", "im_end",
}

// StripSpecialTokens removes chat-template control tokens while leaving
// ordinary markup such as <div> intact. A token is only stripped when its
// body contains one of the characters Tencent's templates use for control
// tokens (`｜`, `▁`) or when it is a known <|...|> marker.
func StripSpecialTokens(input string) string {
	var out strings.Builder
	out.Grow(len(input))

	runes := []rune(input)
	for i := 0; i < len(runes); {
		if runes[i] == '<' {
			// A control token is short; look ahead for a terminating '>'
			// within a bounded rune window.
			windowEnd := i + 96
			if windowEnd > len(runes) {
				windowEnd = len(runes)
			}
			if closeRel := indexRune(runes[i:windowEnd], '>'); closeRel >= 0 {
				inner := string(runes[i+1 : i+closeRel])
				if looksLikeControlToken(inner) {
					i = i + closeRel + 1
					continue
				}
			}
		}
		out.WriteRune(runes[i])
		i++
	}
	return out.String()
}

func looksLikeControlToken(inner string) bool {
	if strings.ContainsRune(inner, '\uFF5C') || strings.ContainsRune(inner, '\u2581') {
		return true
	}
	if len(inner) >= 2 && inner[0] == '|' && inner[len(inner)-1] == '|' {
		name := inner[1 : len(inner)-1]
		name = strings.ToLower(name)
		for _, known := range knownTokenPrefixes {
			if strings.HasPrefix(name, known) {
				return true
			}
		}
	}
	return false
}

func indexRune(runes []rune, target rune) int {
	for i, r := range runes {
		if r == target {
			return i
		}
	}
	return -1
}
