package core

import "testing"

func TestLanguagesHasNoDuplicates(t *testing.T) {
	seen := map[string]bool{}
	names := map[string]bool{}
	for _, l := range Languages {
		if seen[l.Code] {
			t.Fatalf("duplicate code %q", l.Code)
		}
		if names[l.NameEn] {
			t.Fatalf("duplicate name %q", l.NameEn)
		}
		if l.NameKo == "" || l.NameZh == "" {
			t.Fatalf("empty names for %q", l.Code)
		}
		seen[l.Code], names[l.NameEn] = true, true
	}
}

func TestLanguagesCoverModelCard(t *testing.T) {
	if len(Languages) != 38 {
		t.Fatalf("want 38 languages, got %d", len(Languages))
	}
	for _, code := range []string{
		"zh", "en", "fr", "pt", "es", "ja", "tr", "ru", "ar", "ko", "th", "it", "de", "vi",
		"ms", "id", "tl", "hi", "zh-Hant", "pl", "cs", "nl", "km", "my", "fa", "gu", "ur",
		"te", "mr", "he", "bn", "ta", "uk", "bo", "kk", "mn", "ug", "yue",
	} {
		if _, ok := ByCode(code); !ok {
			t.Fatalf("missing %q", code)
		}
	}
}

func TestChineseScriptLanguages(t *testing.T) {
	for _, code := range []string{"zh", "zh-Hant", "yue"} {
		if l, _ := ByCode(code); !l.IsChinese() {
			t.Fatalf("%s should be Chinese script", code)
		}
	}
	for _, code := range []string{"ja", "ko"} {
		if l, _ := ByCode(code); l.IsChinese() {
			t.Fatalf("%s should not be Chinese script", code)
		}
	}
}

func TestDetectCommonScripts(t *testing.T) {
	cases := map[string]string{
		"안녕하세요":              "ko",
		"こんにちは":              "ja",
		"今天天气真好":             "zh",
		"Привет":             "ru",
		"Слава Україні":      "uk",
		"Сәлеметсіз бе":      "kk",
		"Түвшин өөрчлөгдлөө": "mn",
		"שלום עולם":          "he",
		"مرحبا بالعالم":      "ar",
		"چه خبر":             "fa",
		"تم کیسے ہو":         "ur",
		"سالام سىزگە":        "ug",
		"नमस्ते दुनिया":      "hi",
		"নমস্কার":            "bn",
		"வணக்கம்":            "ta",
		"నమస్కారం":           "te",
		"નમસ્તે":             "gu",
		"สวัสดีครับ":         "th",
		"မင်္ဂလာပါ":          "my",
		"សួស្តី":             "km",
		"བཀྲ་ཤིས་བདེ་ལེགས།":  "bo",
	}
	for text, want := range cases {
		if l, ok := Detect(text); !ok || l.Code != want {
			t.Fatalf("Detect(%q) = %v ok=%v, want %s", text, l, ok, want)
		}
	}
	for _, text := range []string{"Hello there", "12345", "Hello world Привет"} {
		if _, ok := Detect(text); ok {
			t.Fatalf("Detect(%q) should fail", text)
		}
	}
}

func TestDetectMixedLatinKeepsScriptLanguages(t *testing.T) {
	// Another detectable script must still win over an embedded Latin word,
	// while Latin-only text stays undetected.
	if l, ok := Detect("이것은 Hello World test 입니다"); !ok || l.Code != "ko" {
		t.Fatalf("hangul should beat latin: %v ok=%v", l, ok)
	}
}

func TestResolveDirection(t *testing.T) {
	cases := []struct {
		name             string
		text             string
		source, target   string
		wantSrc, wantTgt string
		wantSwap         bool
	}{
		{"same as fallback", "안녕하세요", "ko", "en", "ko", "en", false},
		{"detected other", "こんにちは", "ko", "en", "ja", "en", false},
		{"detected equals target", "今天天气真好", "ko", "zh", "zh", "ko", true},
		{"undetected", "Hello world", "ko", "en", "ko", "en", false},
		{"empty", "", "ko", "en", "ko", "en", false},
	}
	for _, tc := range cases {
		src, tgt, swapped := ResolveDirection(tc.text, tc.source, tc.target)
		if src != tc.wantSrc || tgt != tc.wantTgt || swapped != tc.wantSwap {
			t.Fatalf("%s: got (%s, %s, %v), want (%s, %s, %v)",
				tc.name, src, tgt, swapped, tc.wantSrc, tc.wantTgt, tc.wantSwap)
		}
	}
}
