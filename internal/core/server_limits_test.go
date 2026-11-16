package core

import (
	"fmt"
	"strconv"
	"testing"
)

func TestServerLimitsSanitizeAndFormBoundaries(t *testing.T) {
	fields := []struct {
		name, label string
		min, max    int
		setting     func(*ServerSettings) *int
		form        func(*ServerForm) *string
	}{
		{"port", "포트", 1024, 65535, func(s *ServerSettings) *int { return &s.Port }, func(f *ServerForm) *string { return &f.Port }},
		{"context", "컨텍스트 크기", 512, 32768, func(s *ServerSettings) *int { return &s.CtxSize }, func(f *ServerForm) *string { return &f.CtxSize }},
		{"threads", "스레드", 1, 64, func(s *ServerSettings) *int { return &s.Threads }, func(f *ServerForm) *string { return &f.Threads }},
		{"gpu_layers", "GPU 레이어", -1, 256, func(s *ServerSettings) *int { return &s.GPULayers }, func(f *ServerForm) *string { return &f.GPULayers }},
	}
	for _, field := range fields {
		for _, value := range []int{field.min - 1, field.min, field.max, field.max + 1} {
			t.Run(fmt.Sprintf("%s/%d", field.name, value), func(t *testing.T) {
				s := DefaultSettings()
				*field.setting(&s.Server) = value
				fixed := s.Sanitized()
				want := value
				if value < field.min {
					want = field.min
				} else if value > field.max {
					want = field.max
				}
				if got := *field.setting(&fixed.Server); got != want {
					t.Fatalf("Sanitized() = %d, want %d", got, want)
				}

				original := DefaultServerSettings()
				form := NewServerForm(original)
				*field.form(&form) = strconv.Itoa(value)
				applied, err := form.Apply(original)
				if value < field.min || value > field.max {
					wantErr := fmt.Sprintf("%s: %d~%d 범위의 정수를 입력하세요", field.label, field.min, field.max)
					if err == nil || err.Error() != wantErr {
						t.Fatalf("Apply() error = %v, want %q", err, wantErr)
					}
					if applied != (ServerSettings{}) {
						t.Fatalf("Apply() rejected settings = %+v, want zero value", applied)
					}
					return
				}
				if err != nil {
					t.Fatalf("Apply() error = %v", err)
				}
				if got := *field.setting(&applied); got != value {
					t.Fatalf("Apply() = %d, want %d", got, value)
				}
			})
		}
	}
}
