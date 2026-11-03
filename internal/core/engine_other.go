//go:build !windows

package core

import "os/exec"

// hideWindow is a no-op off Windows.
func hideWindow(cmd *exec.Cmd) {}
