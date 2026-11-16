package core

// BuildPromptFromSettings assembles the prompt from settings, keeping one exclusive
// mode priority as the Rust UI did: delimiters, then glossary, then style.
func BuildPromptFromSettings(settings PromptSettings, source, targetCode string) string {
	target, ok := ByCode(targetCode)
	if !ok {
		target = DefaultTarget()
	}
	mode := PromptMode{Style: settings.Style}
	if settings.PreserveDelimiters {
		mode.Delimiters = true
		mode.Style = ""
	}
	if settings.UseGlossary && !mode.Delimiters {
		mode.Pairs = ParseGlossary(settings.Glossary)
	}
	language := PromptLanguage(settings.Language)
	return BuildPrompt(mode, source, target, language)
}
