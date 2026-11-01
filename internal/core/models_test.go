package core

import (
	"path/filepath"
	"strings"
	"testing"
)

func TestCatalogueEntriesAreConsistent(t *testing.T) {
	if _, ok := ModelByID(DefaultModelID); !ok {
		t.Fatal("default id missing")
	}
	if !RecommendedModel().Recommended {
		t.Fatal("recommended flag wrong")
	}
	for _, option := range Catalog {
		if !strings.HasSuffix(option.File, ".gguf") {
			t.Fatalf("%s must be gguf", option.File)
		}
		if option.SizeBytes <= 100_000_000 {
			t.Fatalf("%s suspicious size", option.ID)
		}
		if !strings.Contains(option.DownloadURL("https://huggingface.co"), option.Repo) {
			t.Fatalf("url for %s", option.ID)
		}
		if option.Note == "" {
			t.Fatalf("note for %s", option.ID)
		}
	}
}

func TestBuildsDownloadURLsForHuggingFaceAndMirrors(t *testing.T) {
	option := RecommendedModel()
	if got := option.DownloadURL("https://huggingface.co/"); got !=
		"https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF/resolve/main/Hy-MT2-1.8B-Q4_K_M.gguf?download=true" {
		t.Fatalf("url = %s", got)
	}
	if got := option.DownloadURL("https://hf-mirror.com"); got !=
		"https://hf-mirror.com/tencent/Hy-MT2-1.8B-GGUF/resolve/main/Hy-MT2-1.8B-Q4_K_M.gguf?download=true" {
		t.Fatalf("mirror url = %s", got)
	}
}

func TestFormatsSizes(t *testing.T) {
	if HumanSize(1_133_080_448) != "1.06 GB" {
		t.Fatal("gb format wrong")
	}
	if HumanSize(5*1024*1024) != "5 MB" {
		t.Fatal("mb format wrong")
	}
	if HumanSize(999) != "999 B" {
		t.Fatal("b format wrong")
	}
}

func TestCurlArgsResumeAndVerify(t *testing.T) {
	args := CurlArgs("https://example.com/model.gguf", "C:/models/model.gguf.part")
	hasResume := false
	for i := 0; i < len(args)-1; i++ {
		if args[i] == "-C" && args[i+1] == "-" {
			hasResume = true
		}
	}
	if !hasResume {
		t.Fatal("missing -C -")
	}
	found := false
	for _, arg := range args {
		if arg == "--fail" {
			found = true
		}
	}
	if !found {
		t.Fatal("missing --fail")
	}
	if args[len(args)-1] != "https://example.com/model.gguf" {
		t.Fatal("url must be last")
	}
	for i := 0; i < len(args)-1; i++ {
		if args[i] == "-o" && args[i+1] != "C:/models/model.gguf.part" {
			t.Fatal("-o target wrong")
		}
	}
}

func TestPartPathKeepsTheModelName(t *testing.T) {
	dst := filepath.Join("tmp", "Hy-MT2-1.8B-Q4_K_M.gguf")
	if got := PartPath(dst); got != dst+".part" {
		t.Fatalf("part = %s", got)
	}
}

func TestProgressIsClampedAndReadable(t *testing.T) {
	p := NewDownloadProgress(512, 1024)
	if p.Fraction() != 0.5 || !strings.Contains(p.Label(), "50%") {
		t.Fatalf("p = %v %s", p.Fraction(), p.Label())
	}
	over := NewDownloadProgress(4096, 1024)
	if over.Fraction() != 1.0 {
		t.Fatal("over must clamp to 1.0")
	}
	if NewDownloadProgress(0, 0).Fraction() != 0 {
		t.Fatal("zero total must be 0")
	}
}

func TestRecognizesGgufFilesCaseInsensitively(t *testing.T) {
	if !IsModelFile("a.GGUF") {
		t.Fatal("upper gguf rejected")
	}
	if IsModelFile("a.part") {
		t.Fatal("part accepted")
	}
}
