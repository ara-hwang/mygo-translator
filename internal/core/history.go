package core

import (
	"encoding/json"
	"os"
	"path/filepath"
	"time"
)

// HistorySettings controls opt-in local storage; zero days means unlimited.
type HistorySettings struct {
	Enabled       bool `json:"enabled"`
	RetentionDays int  `json:"retention_days"`
}

func (s HistorySettings) Sanitized() HistorySettings {
	if s.RetentionDays != 0 && s.RetentionDays != 7 && s.RetentionDays != 30 {
		s.RetentionDays = 7
	}
	return s
}

// HistoryEntry is a successfully completed translation.
type HistoryEntry struct {
	CreatedAt   time.Time `json:"created_at"`
	SourceLang  string    `json:"source_lang"`
	TargetLang  string    `json:"target_lang"`
	Source      string    `json:"source"`
	Translation string    `json:"translation"`
}

// PruneHistory keeps entries within a rolling retention window, in stored order.
func PruneHistory(entries []HistoryEntry, days int, now time.Time) []HistoryEntry {
	kept := make([]HistoryEntry, 0, len(entries))
	for _, entry := range entries {
		if days == 0 || !entry.CreatedAt.Before(now.AddDate(0, 0, -days)) {
			kept = append(kept, entry)
		}
	}
	return kept
}

func LoadHistory(path string) ([]HistoryEntry, error) {
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var entries []HistoryEntry
	err = json.Unmarshal(data, &entries)
	return entries, err
}

// SaveHistory replaces the file atomically, with private file permissions.
func SaveHistory(path string, entries []HistoryEntry) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	data, err := json.Marshal(entries)
	if err != nil {
		return err
	}
	f, err := os.CreateTemp(filepath.Dir(path), ".history-*")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if _, err := f.Write(data); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	return os.Rename(f.Name(), path)
}
