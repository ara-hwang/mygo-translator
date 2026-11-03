package core

import (
	"os"
	"path/filepath"
)

// AppPaths resolves locations for settings, models and logs.
//
// Two layouts are supported (matching the Rust app so both share one data
// directory):
//
//   - Portable: if portable.txt sits next to the executable, or a models
//     folder is next to it, everything stays beside the .exe.
//   - Installed: otherwise %LOCALAPPDATA%\HyMT, falling back to ~/.hymt
//     (and the temp folder as a last resort).
type AppPaths struct {
	root string
}

// AppPathsFromRoot uses an explicit root directory.
func AppPathsFromRoot(root string) AppPaths {
	return AppPaths{root: root}
}

// DetectPaths detects the layout for the running executable.
func DetectPaths() AppPaths {
	exeDir := ""
	if exe, err := os.Executable(); err == nil {
		if abs, err := filepath.Abs(exe); err == nil {
			exeDir = filepath.Dir(abs)
		}
	}
	return DetectPathsWith(exeDir, os.Getenv("LOCALAPPDATA"))
}

// DetectPathsWith is the pure detection used by tests and DetectPaths.
func DetectPathsWith(exeDir, localAppData string) AppPaths {
	if exeDir != "" {
		if fileExists(filepath.Join(exeDir, "portable.txt")) ||
			dirExists(filepath.Join(exeDir, "models")) {
			return AppPathsFromRoot(exeDir)
		}
	}
	if localAppData != "" {
		return AppPathsFromRoot(filepath.Join(localAppData, "HyMT"))
	}
	if home, err := os.UserHomeDir(); err == nil && home != "" {
		return AppPathsFromRoot(filepath.Join(home, ".hymt"))
	}
	return AppPathsFromRoot(filepath.Join(os.TempDir(), "hymt"))
}

// Root is the base directory for everything else.
func (p AppPaths) Root() string { return p.root }

// SettingsFile is `settings.json`.
func (p AppPaths) SettingsFile() string {
	return filepath.Join(p.root, "settings.json")
}

// ModelsDir is the folder holding .gguf models.
func (p AppPaths) ModelsDir() string { return filepath.Join(p.root, "models") }

// RuntimeDir is where a llama-server binary can be placed.
func (p AppPaths) RuntimeDir() string { return filepath.Join(p.root, "runtime") }

// LogsDir is the folder for engine logs.
func (p AppPaths) LogsDir() string { return filepath.Join(p.root, "logs") }

// EngineLog is the log file of the managed llama.cpp server.
func (p AppPaths) EngineLog() string {
	return filepath.Join(p.LogsDir(), "llama-server.log")
}

// AppLog is the log file of the application itself.
func (p AppPaths) AppLog() string {
	return filepath.Join(p.LogsDir(), "offline-translator.log")
}

// Ensure creates the directory tree. Safe to call repeatedly.
func (p AppPaths) Ensure() error {
	for _, dir := range []string{p.root, p.ModelsDir(), p.RuntimeDir(), p.LogsDir()} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
	}
	return nil
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}

func dirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}
