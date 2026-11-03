package app

import "testing"

// TestAppFontParses checks the embedded variable TTF registers as a
// family the UI can take.
func TestAppFontParses(t *testing.T) {
	if err := registerAppFont(); err != nil {
		t.Fatalf("RegisterFont: %v", err)
	}
}
