package app

import _ "embed"

//go:embed assets/icon.png
var windowIconPNG []byte

//go:embed assets/tray.png
var trayIconPNG []byte

//go:embed assets/fonts/PretendardVariable.ttf
var appFontTTF []byte
