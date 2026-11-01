package core

import (
	"fmt"
	"strings"
)

// Glossary limits, mirroring the Rust core.
const (
	GlossaryMaxEntries   = 200
	GlossaryMaxTermChars = 120
)

// ParseGlossary parses glossary text, dropping comments, blank lines and
// malformed entries.
func ParseGlossary(text string) [][2]string {
	pairs, _ := ParseGlossaryReport(text)
	return pairs
}

// ParseGlossaryReport parses glossary text and reports why entries were
// skipped, in Korean, one message per skipped line.
func ParseGlossaryReport(text string) ([][2]string, []string) {
	var pairs [][2]string
	var warnings []string

	for index, rawLine := range strings.Split(text, "\n") {
		line := strings.TrimSpace(strings.TrimSuffix(rawLine, "\r"))
		if line == "" || strings.HasPrefix(line, "#") || strings.HasPrefix(line, "//") {
			continue
		}
		source, target, ok := splitGlossaryPair(line)
		if !ok {
			warnings = append(warnings, fmt.Sprintf("%d번째 줄: 구분자(`=>` 또는 탭)가 없습니다.", index+1))
			continue
		}
		if source == "" || target == "" {
			warnings = append(warnings, fmt.Sprintf("%d번째 줄: 원문 또는 번역이 비어 있습니다.", index+1))
			continue
		}
		if len([]rune(source)) > GlossaryMaxTermChars || len([]rune(target)) > GlossaryMaxTermChars {
			warnings = append(warnings, fmt.Sprintf("%d번째 줄: 용어가 너무 깁니다(최대 %d자).", index+1, GlossaryMaxTermChars))
			continue
		}
		if len(pairs) >= GlossaryMaxEntries {
			warnings = append(warnings, fmt.Sprintf("용어집은 최대 %d개까지 사용합니다.", GlossaryMaxEntries))
			break
		}
		// Later definitions win; keeping the list free of duplicates keeps
		// the prompt short.
		replaced := false
		for i := range pairs {
			if pairs[i][0] == source {
				pairs[i][1] = target
				replaced = true
				break
			}
		}
		if !replaced {
			pairs = append(pairs, [2]string{source, target})
		}
	}
	return pairs, warnings
}

func splitGlossaryPair(line string) (string, string, bool) {
	if i := strings.Index(line, "=>"); i >= 0 {
		return strings.TrimSpace(line[:i]), strings.TrimSpace(line[i+2:]), true
	}
	if i := strings.IndexByte(line, '\t'); i >= 0 {
		return strings.TrimSpace(line[:i]), strings.TrimSpace(line[i+1:]), true
	}
	return "", "", false
}

// FormatGlossary renders pairs back into the canonical "source=>target" form.
func FormatGlossary(pairs [][2]string) string {
	parts := make([]string, len(pairs))
	for i, pair := range pairs {
		parts[i] = fmt.Sprintf("%s => %s", pair[0], pair[1])
	}
	return strings.Join(parts, "\n")
}
