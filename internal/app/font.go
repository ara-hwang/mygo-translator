package app

import "github.com/egoist/mygo/ui"

// registerAppFont registers the embedded Pretendard Variable font with
// the UI's text system; the settings show its family name.
func registerAppFont() error {
	return ui.RegisterFont(appFontTTF, appFontFamily)
}
