package app

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"offline-translator/internal/core"
)

func TestEngineStatusColor(t *testing.T) {
	for _, theme := range []*ui.Theme{ui.LightTheme(), ui.DarkTheme()} {
		for _, tc := range []struct {
			kind core.EngineStateKind
			want ui.Color
		}{
			{core.EngineReady, theme.Success},
			{core.EngineStarting, theme.Warning},
			{core.EngineFailed, theme.Danger},
			{core.EngineStopped, theme.TextMuted},
			{core.EngineStateKind(-1), theme.TextMuted},
		} {
			if got := engineStatusColor(tc.kind, theme); got != tc.want {
				t.Errorf("dark=%v kind=%v: got %v, want %v", theme.Dark, tc.kind, got, tc.want)
			}
		}
	}
}
