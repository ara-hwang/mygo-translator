package core

import (
	"os"
	"path/filepath"
	"testing"
)

func TestPortableModeUsedWhenMarkerExists(t *testing.T) {
	dir := t.TempDir()
	os.WriteFile(filepath.Join(dir, "portable.txt"), []byte(""), 0o644)
	resolved := DetectPathsWith(dir, "C:/AppData")
	if resolved.Root() != dir {
		t.Fatalf("root = %s", resolved.Root())
	}
}

func TestLocalModelsFolderAlsoMeansPortable(t *testing.T) {
	dir := t.TempDir()
	os.MkdirAll(filepath.Join(dir, "models"), 0o755)
	resolved := DetectPathsWith(dir, "C:/AppData")
	if resolved.Root() != dir {
		t.Fatalf("root = %s", resolved.Root())
	}
}

func TestInstalledLayoutUsesLocalAppData(t *testing.T) {
	dir := t.TempDir()
	resolved := DetectPathsWith(dir, "C:/AppData")
	if resolved.Root() != filepath.Join("C:/AppData", "offline-translator") {
		t.Fatalf("root = %s", resolved.Root())
	}
	if filepath.Base(resolved.SettingsFile()) != "settings.json" {
		t.Fatalf("settings = %s", resolved.SettingsFile())
	}
	if filepath.Base(resolved.ModelsDir()) != "models" {
		t.Fatalf("models = %s", resolved.ModelsDir())
	}
}

func TestInstalledLayoutLeavesLegacyDataUntouched(t *testing.T) {
	local := t.TempDir()
	legacy := filepath.Join(local, "HyMT")
	if err := os.MkdirAll(legacy, 0o755); err != nil {
		t.Fatal(err)
	}
	oldSettings := filepath.Join(legacy, "settings.json")
	if err := os.WriteFile(oldSettings, []byte("legacy settings"), 0o644); err != nil {
		t.Fatal(err)
	}
	paths := DetectPathsWith(t.TempDir(), local)
	if err := paths.Ensure(); err != nil {
		t.Fatal(err)
	}
	if paths.Root() != filepath.Join(local, "offline-translator") {
		t.Fatalf("root = %s", paths.Root())
	}
	if data, err := os.ReadFile(oldSettings); err != nil || string(data) != "legacy settings" {
		t.Fatalf("legacy settings changed: %q, %v", data, err)
	}
	if _, err := os.Stat(paths.SettingsFile()); !os.IsNotExist(err) {
		t.Fatalf("legacy settings must not be copied: %v", err)
	}
}

func TestEnsureCreatesEveryDirectory(t *testing.T) {
	paths := AppPathsFromRoot(filepath.Join(t.TempDir(), "root"))
	if err := paths.Ensure(); err != nil {
		t.Fatal(err)
	}
	if !dirExists(paths.ModelsDir()) || !dirExists(paths.RuntimeDir()) || !dirExists(paths.LogsDir()) {
		t.Fatal("directories missing")
	}
}
