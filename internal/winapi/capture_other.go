//go:build !windows

// Package winapi is only used on Windows; other platforms get no capture.
package winapi

import "fmt"

// CaptureSelected is unsupported off Windows.
func CaptureSelected() (string, error) {
	return "", fmt.Errorf("이 플랫폼에서는 선택 텍스트 캡처를 지원하지 않습니다.")
}
