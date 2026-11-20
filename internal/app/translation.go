package app

import (
	"context"
	"fmt"
	"strings"
	"time"

	"offline-translator/internal/core"
)

// StartTranslate streams a translation into OutputText.
func (a *App) StartTranslate() {
	a.Mu.Lock()
	if a.Streaming {
		a.Mu.Unlock()
		return
	}
	source := a.SourceText
	targetCode := a.settings.TargetLang
	promptSettings := a.settings.Prompt
	generation := a.settings.Generation
	persist := false
	// With the source select on 자동 the direction is resolved from the text
	// before the prompt is built. On a detect failure the stored source and
	// target pair stays as it is.
	translateNotice := ""
	if a.SourceSelName == autoSourceLabel && strings.TrimSpace(source) != "" {
		if _, ok := core.Detect(source); !ok {
			translateNotice = "소스 언어를 인식하지 못했습니다. 원문 언어를 직접 선택해 주세요."
		} else {
			oldTarget := a.settings.TargetLang
			sourceCode, newTarget, swapped := core.ResolveDirection(source, a.settings.SourceLang, a.settings.TargetLang)
			if sourceCode != a.settings.SourceLang || newTarget != oldTarget {
				persist = true
			}
			a.settings.SourceLang = sourceCode
			a.settings.TargetLang = newTarget
			targetCode = newTarget
			if newTarget != oldTarget {
				a.TargetSelName = languageName(newTarget)
			}
			if swapped {
				translateNotice = "감지된 언어가 타깃과 같아 방향을 바꿉니다."
			}
		}
	}
	settings := a.settings
	a.Mu.Unlock()
	if persist {
		go settings.Save(a.paths.SettingsFile())
	}

	// An empty input is expected; the source editor's placeholder carries
	// the hint, not the notice bar.
	if strings.TrimSpace(source) == "" {
		return
	}

	if ok, message := core.ValidateSource(source); !ok {
		a.Mu.Lock()
		a.Notice = message
		a.Mu.Unlock()
		a.Invalidate()
		return
	}
	prompt := core.BuildPromptFromSettings(promptSettings, source, targetCode)
	endpoint := core.LocalhostEndpoint(a.settings.Server.Port)
	const model = "hy-mt2"

	ctx, cancel := context.WithCancel(context.Background())
	a.cancelMu.Lock()
	a.translateC = cancel
	a.cancelMu.Unlock()

	a.Mu.Lock()
	a.Streaming = true
	a.OutputText = ""
	a.OutputStats = ""
	a.Notice = translateNotice
	a.Mu.Unlock()
	a.Invalidate()

	go func() {
		defer cancel()
		var raw strings.Builder
		onDelta := func(delta string) {
			raw.WriteString(delta)
			a.Mu.Lock()
			a.OutputText = core.NormalizeOutput(raw.String())
			a.Mu.Unlock()
			a.Invalidate()
		}
		result, err := core.TranslateStream(ctx, endpoint, model, core.GenParamsFrom(generation), prompt, onDelta)
		a.Mu.Lock()
		a.Streaming = false
		a.OutputStats = ""
		if err != nil {
			if chat, ok := err.(*core.ChatError); ok {
				switch chat.Kind {
				case core.ChatErrorHTTP:
					if strings.Contains(chat.Error(), "connect") || strings.Contains(chat.Error(), "refused") {
						a.Notice = "모델 서버에 연결할 수 없습니다. 모델 탭에서 엔진을 시작하세요."
					} else {
						a.Notice = chat.Error()
					}
				default:
					a.Notice = chat.Error()
				}
			} else {
				a.Notice = err.Error()
			}
			if result != nil && result.Text != "" {
				a.OutputText = result.Text
			}
		} else if result != nil {
			a.OutputText = result.Text
			if !result.Cancelled && settings.History.Enabled && a.settings.History.Enabled && strings.TrimSpace(result.Text) != "" && !a.historyFailed {
				a.history = append([]core.HistoryEntry{{CreatedAt: time.Now(), SourceLang: settings.SourceLang, TargetLang: targetCode, Source: source, Translation: result.Text}}, a.history...)
				a.saveHistoryLocked()
			}
			speed := ""
			if result.TokensPerSecond > 0 {
				speed = fmt.Sprintf(" · %.1f tok/s", result.TokensPerSecond)
			}
			a.OutputStats = fmt.Sprintf("%d ms · %d tok%s", result.TotalMs, result.CompletionTokens, speed)
		}
		a.Mu.Unlock()
		a.Invalidate()
	}()
}

// CancelTranslate cancels the running translation; partial text stays.
func (a *App) CancelTranslate() {
	a.cancelMu.Lock()
	cancel := a.translateC
	a.cancelMu.Unlock()
	if cancel != nil {
		cancel()
	}
}
