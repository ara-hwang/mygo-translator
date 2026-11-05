package core

import (
	"path/filepath"
	"testing"
)

func TestServerFormRoundTrip(t *testing.T) {
	s := DefaultServerSettings()
	path := filepath.Join(t.TempDir(), "llama-server")
	model := filepath.Join(t.TempDir(), "model.gguf")
	s.RuntimePath, s.ModelPath = &path, &model
	got, err := NewServerForm(s).Apply(s)
	if err != nil || got.Port != s.Port || got.CtxSize != s.CtxSize || got.Threads != s.Threads || got.GPULayers != s.GPULayers || got.AutoStart != s.AutoStart || *got.RuntimePath != path || *got.ModelPath != model {
		t.Fatalf("round trip: %+v, %v", got, err)
	}
}

func TestServerFormValidation(t *testing.T) {
	for _, tc := range []struct {
		name string
		edit func(*ServerForm)
	}{
		{"empty", func(f *ServerForm) { f.Port = "" }},
		{"fraction", func(f *ServerForm) { f.Threads = "1.5" }},
		{"port low", func(f *ServerForm) { f.Port = "1023" }},
		{"port high", func(f *ServerForm) { f.Port = "65536" }},
		{"context low", func(f *ServerForm) { f.CtxSize = "511" }},
		{"context high", func(f *ServerForm) { f.CtxSize = "32769" }},
		{"threads low", func(f *ServerForm) { f.Threads = "0" }},
		{"threads high", func(f *ServerForm) { f.Threads = "65" }},
		{"gpu low", func(f *ServerForm) { f.GPULayers = "-2" }},
		{"gpu high", func(f *ServerForm) { f.GPULayers = "257" }},
		{"relative runtime", func(f *ServerForm) { f.RuntimePath = "llama-server" }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := DefaultServerSettings()
			f := NewServerForm(s)
			tc.edit(&f)
			if _, err := f.Apply(s); err == nil {
				t.Fatal("invalid form accepted")
			}
		})
	}
}

func TestServerFormBoundsAndAutoDiscovery(t *testing.T) {
	s := DefaultServerSettings()
	for _, values := range [][4]string{{"1024", "512", "1", "-1"}, {"65535", "32768", "64", "256"}} {
		f := ServerForm{Port: values[0], CtxSize: values[1], Threads: values[2], GPULayers: values[3], RuntimePath: "  "}
		got, err := f.Apply(s)
		if err != nil || got.RuntimePath != nil || got.AutoStart {
			t.Fatalf("bounds: %+v, %v", got, err)
		}
	}
}
