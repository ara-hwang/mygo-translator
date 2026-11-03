//go:build windows

// Package winapi holds the small pieces of Win32 the desktop needs that
// mygo does not wrap: synthesizing a copy shortcut into the application the
// user is working in, so a hotkey can capture selected text.
package winapi

import (
	"syscall"
	"unsafe"
)

var (
	user32        = syscall.NewLazyDLL("user32.dll")
	procSendInput = user32.NewProc("SendInput")
)

const (
	inputKeyboard  = 1
	keyeventfKeyup = 0x0002
	vkControl      = 0x11
	vkHome         = 0x24
	vkEnd          = 0x23
	vkC            = 0x43
)

// keybdInput mirrors KEYBDINPUT.
type keybdInput struct {
	WVK   uint16
	Scan  uint16
	Flags uint32
	Time  uint32
	Extra uintptr
}

// inputUnion mirrors INPUT on amd64 (40 bytes total, the mouse variant is
// the largest union member).
type inputUnion struct {
	Type uint32
	_    [4]byte
	Kbd  keybdInput
	_    [8]byte
}

var inputSize = uint32(unsafe.Sizeof(inputUnion{}))

// sendKey sends one key press or release.
func sendKey(vk uint16, up bool) {
	var in inputUnion
	in.Type = inputKeyboard
	in.Kbd = keybdInput{WVK: vk}
	if up {
		in.Kbd.Flags = keyeventfKeyup
	}
	procSendInput.Call(1, uintptr(unsafe.Pointer(&in)), uintptr(inputSize))
}

// sendChord presses one virtual key with Ctrl held.
func sendChord(vk uint16) {
	sendKey(vkControl, false)
	sendKey(vk, false)
	sendKey(vk, true)
	sendKey(vkControl, true)
}

// CopySelection sends Ctrl+C to the focused application.
func CopySelection() error {
	sendChord(vkC)
	return nil
}

// ClearSelection collapses the selection with Home then End, so a second
// capture never returns the stale selection.
func ClearSelection() error {
	sendKey(vkHome, false)
	sendKey(vkHome, true)
	sendKey(vkHome, false)
	sendKey(vkEnd, false)
	sendKey(vkEnd, true)
	return nil
}
