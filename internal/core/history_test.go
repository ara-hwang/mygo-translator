package core

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestHistoryRetention(t *testing.T) {
	now := time.Date(2026, 11, 5, 12, 0, 0, 0, time.UTC)
	entries := []HistoryEntry{{CreatedAt: now}, {CreatedAt: now.AddDate(0, 0, -7)}, {CreatedAt: now.AddDate(0, 0, -30)}, {CreatedAt: now.AddDate(0, 0, -31)}}
	for _, tc := range []struct{ days, want int }{{7, 2}, {30, 3}, {0, 4}} {
		if got := len(PruneHistory(entries, tc.days, now)); got != tc.want {
			t.Errorf("days=%d: got %d, want %d", tc.days, got, tc.want)
		}
	}
}

func TestHistoryPersistence(t *testing.T) {
	path := filepath.Join(t.TempDir(), "data", "history.json")
	if entries, err := LoadHistory(path); err != nil || len(entries) != 0 {
		t.Fatalf("missing: %v", err)
	}
	entry := HistoryEntry{CreatedAt: time.Now().UTC(), SourceLang: "en", TargetLang: "ko", Source: "hello", Translation: "안녕"}
	if err := SaveHistory(path, []HistoryEntry{entry}); err != nil {
		t.Fatal(err)
	}
	entries, err := LoadHistory(path)
	if err != nil || len(entries) != 1 || entries[0] != entry {
		t.Fatalf("round trip: %v %v", entries, err)
	}
	if err := SaveHistory(path, nil); err != nil {
		t.Fatal(err)
	}
	if entries, err := LoadHistory(path); err != nil || len(entries) != 0 {
		t.Fatalf("clear: %v", err)
	}
	if err := os.WriteFile(path, []byte("broken"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := LoadHistory(path); err == nil {
		t.Fatal("expected corrupt file error")
	}
}

func TestHistorySettings(t *testing.T) {
	s := DefaultSettings()
	if s.History.Enabled || s.History.RetentionDays != 7 {
		t.Fatal(s.History)
	}
	s.History = HistorySettings{Enabled: true, RetentionDays: 12}
	if got := s.Sanitized().History; !got.Enabled || got.RetentionDays != 7 {
		t.Fatal(got)
	}
	path := filepath.Join(t.TempDir(), "settings.json")
	if err := os.WriteFile(path, []byte(`{}`), 0o600); err != nil {
		t.Fatal(err)
	}
	loaded, warning := LoadOrDefault(path)
	if warning != "" || loaded.History.Enabled || loaded.History.RetentionDays != 7 {
		t.Fatal(loaded.History, warning)
	}
}
