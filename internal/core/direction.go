package core

// ResolveDirection maps the source side of a translation onto concrete
// language codes from the text itself.
//
// `sourceCode` is the fallback that stands when the sample is empty or
// undetectable. When the detected source equals `targetCode` the direction
// would translate the text into its own language, so the pair is exchanged
// instead: the result goes into the former source language. The caller
// guarantees `sourceCode != targetCode` strictly, so the exchanged pair is
// never degenerate either.
func ResolveDirection(text, sourceCode, targetCode string) (src, tgt string, swapped bool) {
	src, tgt = sourceCode, targetCode
	lang, ok := Detect(text)
	if !ok || lang.Code == sourceCode {
		return src, tgt, false
	}
	if lang.Code == targetCode {
		return targetCode, sourceCode, true
	}
	return lang.Code, targetCode, false
}
