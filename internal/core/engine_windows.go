//go:build windows

package core

import (
	"os/exec"

	"golang.org/x/sys/windows"
)

// hideWindow keeps the llama-server console window hidden; the app is a GUI.
func hideWindow(cmd *exec.Cmd) {
	if cmd.SysProcAttr == nil {
		cmd.SysProcAttr = &windows.SysProcAttr{}
	}
	cmd.SysProcAttr.CreationFlags |= windows.CREATE_NO_WINDOW
}
