package app

import (
	"fmt"
	"path/filepath"
	"time"

	"github.com/egoist/mygo/ui"
	"offline-translator/internal/core"
)

func (a *App) historyPath() string {
	return filepath.Join(filepath.Dir(a.paths.SettingsFile()), "history.json")
}

func historyRetentionName(days int) string {
	switch days {
	case 0:
		return "무제한"
	case 30:
		return "최근 30일"
	default:
		return "최근 7일"
	}
}

// Caller holds Mu, or is initializing the application.
func (a *App) saveHistoryLocked() {
	a.history = core.PruneHistory(a.history, a.settings.History.RetentionDays, time.Now())
	if err := core.SaveHistory(a.historyPath(), a.history); err != nil {
		a.Notice = fmt.Sprintf("히스토리를 저장하지 못했습니다: %v", err)
	}
}

func (a *App) pruneHistoryLocked() {
	kept := core.PruneHistory(a.history, a.settings.History.RetentionDays, time.Now())
	if len(kept) != len(a.history) {
		a.history = kept
		a.saveHistoryLocked()
	}
}

func (a *App) historyView(c *ui.Context) {
	a.Mu.Lock()
	a.pruneHistoryLocked()
	entries := append([]core.HistoryEntry(nil), a.history...)
	enabled := a.settings.History.Enabled
	a.Mu.Unlock()
	ui.Column(c).Fill().Padding(16).Gap(12).Children(func() {
		ui.Text(c, "번역 히스토리").FontSize(18).Bold()
		if !enabled {
			ui.Text(c, "저장 꺼짐 · 설정에서 활성화할 수 있습니다.")
		}
		ui.Text(c, "원문과 번역문은 이 컴퓨터에만 저장됩니다.").FontSize(12)
		if ui.Button(c, "히스토리 전체 삭제").Clicked() {
			a.Mu.Lock()
			if err := core.SaveHistory(a.historyPath(), nil); err != nil {
				a.Notice = fmt.Sprintf("히스토리를 삭제하지 못했습니다: %v", err)
			} else {
				a.history = nil
				a.historyFailed = false
			}
			a.Mu.Unlock()
		}
		ui.Scroll(c).Grow(1).Gap(12).Children(func() {
			if len(entries) == 0 {
				ui.Text(c, "저장된 번역이 없습니다.")
			}
			for _, entry := range entries {
				ui.Column(c).Gap(4).Children(func() {
					ui.Text(c, entry.CreatedAt.Local().Format("2006-01-02 15:04")+" · "+languageName(entry.SourceLang)+" → "+languageName(entry.TargetLang)).FontSize(12)
					ui.Text(c, entry.Source)
					ui.Text(c, entry.Translation)
					if !a.isStreaming() && ui.Button(c, "번역 화면에서 열기").Clicked() {
						a.Mu.Lock()
						a.SourceText, a.OutputText = entry.Source, entry.Translation
						a.OutputStats = ""
						a.settings.SourceLang, a.settings.TargetLang = entry.SourceLang, entry.TargetLang
						a.settings.AutoSource = false
						a.SourceSelName, a.TargetSelName = languageName(entry.SourceLang), languageName(entry.TargetLang)
						a.Tab = tabTranslate
						a.Mu.Unlock()
					}
				})
			}
		})
	})
}
