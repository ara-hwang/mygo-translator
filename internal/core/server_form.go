package core

import (
	"fmt"
	"path/filepath"
	"strconv"
	"strings"
)

// ServerForm keeps incomplete user input separate from the active configuration.
type ServerForm struct {
	RuntimePath, Port, CtxSize, Threads, GPULayers string
	AutoStart                                      bool
}

// NewServerForm formats persisted launch settings for editing.
func NewServerForm(s ServerSettings) ServerForm {
	f := ServerForm{Port: strconv.Itoa(s.Port), CtxSize: strconv.Itoa(s.CtxSize), Threads: strconv.Itoa(s.Threads), GPULayers: strconv.Itoa(s.GPULayers), AutoStart: s.AutoStart}
	if s.RuntimePath != nil {
		f.RuntimePath = *s.RuntimePath
	}
	return f
}

// Apply validates every field before returning a replacement configuration.
func (f ServerForm) Apply(s ServerSettings) (ServerSettings, error) {
	fields := []struct {
		label, text string
		min, max    int
		dest        *int
	}{
		{"포트", f.Port, 1024, 65535, &s.Port},
		{"컨텍스트 크기", f.CtxSize, 512, 32768, &s.CtxSize},
		{"스레드", f.Threads, 1, 64, &s.Threads},
		{"GPU 레이어", f.GPULayers, -1, 256, &s.GPULayers},
	}
	for _, field := range fields {
		n, err := strconv.Atoi(strings.TrimSpace(field.text))
		if err != nil || n < field.min || n > field.max {
			return ServerSettings{}, fmt.Errorf("%s: %d~%d 범위의 정수를 입력하세요", field.label, field.min, field.max)
		}
		*field.dest = n
	}
	s.RuntimePath = nil
	if path := strings.TrimSpace(f.RuntimePath); path != "" {
		if !filepath.IsAbs(path) {
			return ServerSettings{}, fmt.Errorf("런타임 경로는 절대 경로로 입력하세요")
		}
		s.RuntimePath = &path
	}
	s.AutoStart = f.AutoStart
	return s, nil
}
