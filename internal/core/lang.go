package core

// LanguageDir lists the languages Hy-MT2 was trained to translate between.
//
// The table mirrors the model card exactly (33 languages plus the dialect and
// ethnic variants Tencent ships: Traditional Chinese, Cantonese, Tibetan,
// Kazakh, Mongolian and Uyghur). Prompts must use the *full* language name,
// never the abbreviation, and the Chinese template needs the Chinese name
// while the English template needs the English one.
type Language struct {
	Code   string // stable identifier used in settings and the UI
	NameEn string // full English name (English prompt template)
	NameZh string // full Chinese name (Chinese prompt template)
	NameKo string // name shown in the Korean UI
}

// IsChinese reports whether the language is written in Chinese script, which
// decides whether the Chinese prompt template is preferred when translating
// into it.
func (l Language) IsChinese() bool {
	return l.Code == "zh" || l.Code == "zh-Hant" || l.Code == "yue"
}

// Languages is every language supported by Hy-MT2, in model-card order.
var Languages = []Language{
	{"ko", "Korean", "韩语", "한국어"},
	{"en", "English", "英语", "영어"},
	{"ja", "Japanese", "日语", "일본어"},
	{"zh", "Chinese", "中文", "중국어(간체)"},
	{"zh-Hant", "Traditional Chinese", "繁体中文", "중국어(번체)"},
	{"yue", "Cantonese", "粤语", "광둥어"},
	{"de", "German", "德语", "독일어"},
	{"fr", "French", "法语", "프랑스어"},
	{"es", "Spanish", "西班牙语", "스페인어"},
	{"pt", "Portuguese", "葡萄牙语", "포르투갈어"},
	{"it", "Italian", "意大利语", "이탈리아어"},
	{"ru", "Russian", "俄语", "러시아어"},
	{"uk", "Ukrainian", "乌克兰语", "우크라이나어"},
	{"pl", "Polish", "波兰语", "폴란드어"},
	{"cs", "Czech", "捷克语", "체코어"},
	{"nl", "Dutch", "荷兰语", "네덜란드어"},
	{"tr", "Turkish", "土耳其语", "터키어"},
	{"ar", "Arabic", "阿拉伯语", "아랍어"},
	{"he", "Hebrew", "希伯来语", "히브리어"},
	{"fa", "Persian", "波斯语", "페르시아어"},
	{"hi", "Hindi", "印地语", "힌디어"},
	{"bn", "Bengali", "孟加拉语", "벵골어"},
	{"ta", "Tamil", "泰米尔语", "타밀어"},
	{"te", "Telugu", "泰卢固语", "텔루구어"},
	{"mr", "Marathi", "马拉地语", "마라티어"},
	{"gu", "Gujarati", "古吉拉特语", "구자라트어"},
	{"ur", "Urdu", "乌尔都语", "우르두어"},
	{"th", "Thai", "泰语", "태국어"},
	{"vi", "Vietnamese", "越南语", "베트남어"},
	{"id", "Indonesian", "印尼语", "인도네시아어"},
	{"ms", "Malay", "马来语", "말레이어"},
	{"tl", "Filipino", "菲律宾语", "필리핀어"},
	{"km", "Khmer", "高棉语", "크메르어"},
	{"my", "Burmese", "缅甸语", "미얀마어"},
	{"bo", "Tibetan", "藏语", "티베트어"},
	{"kk", "Kazakh", "哈萨克语", "카자흐어"},
	{"mn", "Mongolian", "蒙古语", "몽골어"},
	{"ug", "Uyghur", "维吾尔语", "위구르어"},
}

// ByCode looks a language up by its stable code.
func ByCode(code string) (Language, bool) {
	for _, lang := range Languages {
		if lang.Code == code {
			return lang, true
		}
	}
	return Language{}, false
}

// DefaultSource is the source language on a fresh install.
func DefaultSource() Language { l, _ := ByCode("ko"); return l }

// DefaultTarget is the target language on a fresh install.
func DefaultTarget() Language { l, _ := ByCode("en"); return l }

// Detect does a best-effort detection of the language of a text sample.
//
// Intentionally conservative: it only recognizes scripts that can be told
// apart cheaply and returns false for everything else, where the UI keeps
// the current selection.
func Detect(text string) (Language, bool) {
	hangul, han, kana, cyrillic, latin := 0, 0, 0, 0, 0
	for _, ch := range text {
		switch {
		case (ch >= 0xAC00 && ch <= 0xD7A3) || (ch >= 0x1100 && ch <= 0x11FF) || (ch >= 0x3130 && ch <= 0x318F):
			hangul++
		case ch >= 0x3040 && ch <= 0x30FF:
			kana++
		case ch >= 0x4E00 && ch <= 0x9FFF:
			han++
		case ch >= 0x0400 && ch <= 0x04FF:
			cyrillic++
		case (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z'):
			latin++
		}
	}
	// Japanese text always carries kana next to kanji; Korean text is dominated
	// by hangul syllables; Chinese text is han only.
	if kana > 0 {
		return ByCode("ja")
	}
	if hangul > 0 && hangul >= han {
		return ByCode("ko")
	}
	if han > 0 && han > hangul {
		return ByCode("zh")
	}
	if cyrillic > 0 && cyrillic >= latin {
		// Cyrillic is shared; Ukrainian has distinctive letters.
		for _, ch := range text {
			if ch == 'і' || ch == 'ї' || ch == 'є' || ch == 'ґ' {
				return ByCode("uk")
			}
		}
		return ByCode("ru")
	}
	return Language{}, false
}
