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
	if resolved.Root() != filepath.Join("C:/AppData", "HyMT") {
		t.Fatalf("root = %s", resolved.Root())
	}
	if filepath.Base(resolved.SettingsFile()) != "settings.json" {
		t.Fatalf("settings = %s", resolved.SettingsFile())
	}
	if filepath.Base(resolved.ModelsDir()) != "models" {
		t.Fatalf("models = %s", resolved.ModelsDir())
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
