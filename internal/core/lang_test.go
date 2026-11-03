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
	cases := map[string]string{"안녕하세요": "ko", "こんにちは": "ja", "今天天气真好": "zh", "Привет": "ru"}
	for text, want := range cases {
		if l, ok := Detect(text); !ok || l.Code != want {
			t.Fatalf("Detect(%q) = %v ok=%v, want %s", text, l, ok, want)
		}
	}
	for _, text := range []string{"Hello there", "12345"} {
		if _, ok := Detect(text); ok {
			t.Fatalf("Detect(%q) should fail", text)
		}
	}
}
