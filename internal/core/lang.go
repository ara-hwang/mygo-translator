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
// apart cheaply by Unicode ranges — plus the member languages of the shared
// Cyrillic and Arabic scripts via their signature letters — and returns
// false for everything else, where the UI keeps the current selection.
// Latin-script languages deliberately stay undetected: their letters are
// shared across many languages and word-list heuristics would be unreliable.
func Detect(text string) (Language, bool) {
	var (
		hangul, kana, han, cyrillic, arabic, hebrew  int
		devanagari, bengali, tamil, telugu, gujarati int
		thai, myanmar, khmer, tibetan, latin         int
	)
	for _, ch := range text {
		switch {
		case (ch >= 0xAC00 && ch <= 0xD7A3) || (ch >= 0x1100 && ch <= 0x11FF) || (ch >= 0x3130 && ch <= 0x318F):
			hangul++
		case ch >= 0x3040 && ch <= 0x30FF:
			kana++
		case ch >= 0x4E00 && ch <= 0x9FFF:
			han++
		case (ch >= 0x0400 && ch <= 0x04FF) || (ch >= 0x0500 && ch <= 0x052F):
			cyrillic++
		case (ch >= 0x0600 && ch <= 0x06FF) || (ch >= 0x0750 && ch <= 0x077F):
			arabic++
		case ch >= 0x0590 && ch <= 0x05FF:
			hebrew++
		case ch >= 0x0900 && ch <= 0x097F:
			devanagari++
		case ch >= 0x0980 && ch <= 0x09FF:
			bengali++
		case ch >= 0x0A80 && ch <= 0x0AFF:
			gujarati++
		case ch >= 0x0B80 && ch <= 0x0BFF:
			tamil++
		case ch >= 0x0C00 && ch <= 0x0C7F:
			telugu++
		case ch >= 0x0E00 && ch <= 0x0E7F:
			thai++
		case ch >= 0x1000 && ch <= 0x109F:
			myanmar++
		case ch >= 0x1780 && ch <= 0x17FF:
			khmer++
		case ch >= 0x0F00 && ch <= 0x0FFF:
			tibetan++
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
	// The remaining letters never resolve exclusively on their own: each
	// branch must at least match the latin count so one quoted word in
	// mostly-Latin text does not win the whole sample.
	if cyrillic > 0 && cyrillic >= latin {
		return resolveCyrillic(text)
	}
	if arabic > 0 && arabic >= latin {
		return resolveArabic(text)
	}
	if hebrew > 0 && hebrew >= latin {
		return ByCode("he")
	}
	if thai > 0 && thai >= latin {
		return ByCode("th")
	}
	// Devanagari covers both Hindi and Marathi; they share the script, so
	// the more common Hindi stands in.
	if devanagari > 0 && devanagari >= latin {
		return ByCode("hi")
	}
	if bengali > 0 && bengali >= latin {
		return ByCode("bn")
	}
	if tamil > 0 && tamil >= latin {
		return ByCode("ta")
	}
	if telugu > 0 && telugu >= latin {
		return ByCode("te")
	}
	if gujarati > 0 && gujarati >= latin {
		return ByCode("gu")
	}
	if myanmar > 0 && myanmar >= latin {
		return ByCode("my")
	}
	if khmer > 0 && khmer >= latin {
		return ByCode("km")
	}
	if tibetan > 0 && tibetan >= latin {
		return ByCode("bo")
	}
	return Language{}, false
}

// resolveCyrillic tells the Cyrillic-script members apart via their
// signature letters, falling back to Russian. Kazakh catches its letters
// before plain і, which is shared with Ukrainian.
func resolveCyrillic(text string) (Language, bool) {
	var ukrainian, kazakh, mongolian, ukrainianI bool
	for _, ch := range text {
		switch ch {
		case 0x0457, 0x0454, 0x0491: // ї є ґ — Ukrainian-only
			ukrainian = true
		case 0x04D9, 0x0493, 0x049B, 0x04B1, 0x04BB, 0x04A3: // ә ғ қ ұ һ ң — Kazakh
			kazakh = true
		case 0x04E9, 0x04AF: // ө ү — Mongolian Cyrillic
			mongolian = true
		case 0x0456: // і — Kazakh too, but only without the letters above
			ukrainianI = true
		}
	}
	switch {
	case ukrainian:
		return ByCode("uk")
	case kazakh:
		return ByCode("kk")
	case mongolian:
		return ByCode("mn")
	case ukrainianI:
		return ByCode("uk")
	}
	return ByCode("ru")
}

// resolveArabic tells the Arabic-script members apart via their signature
// letters, falling back to Arabic. The Uyghur and Urdu letters are checked
// before the Persian ones because the Persian signatures appear in those
// alphabets as well.
func resolveArabic(text string) (Language, bool) {
	var uyghur, urdu, persian bool
	for _, ch := range text {
		switch ch {
		case 0x06CB, 0x06D0, 0x06D5: // ۋ ې ە — Uyghur-only
			uyghur = true
		case 0x0679, 0x0688, 0x0691, 0x06BA, 0x06C1, 0x06D2: // ٹ ڈ ڑ ں ہ ے — Urdu-only
			urdu = true
		case 0x06AF, 0x0686, 0x067E, 0x0698: // گ چ پ ژ — Persian signature
			persian = true
		}
	}
	switch {
	case uyghur:
		return ByCode("ug")
	case urdu:
		return ByCode("ur")
	case persian:
		return ByCode("fa")
	}
	return ByCode("ar")
}
