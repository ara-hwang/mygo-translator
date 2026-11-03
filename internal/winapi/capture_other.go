//go:build !windows

// Package winapi is only used on Windows; other platforms get no-op
// capture.
package winapi

// CopySelection is a no-op off Windows.
func CopySelection() error { return nil }
