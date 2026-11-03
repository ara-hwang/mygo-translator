package core

import (
	"fmt"
	"path/filepath"
	"strings"
)

// ModelOption is one downloadable model file.
type ModelOption struct {
	// ID is the stable identifier used by the UI.
	ID string
	// Family is the model family, e.g. "Hy-MT2-1.8B".
	Family string
	// Repo is the Hugging Face repository.
	Repo string
	// File is the exact file name inside the repository.
	File string
	// SizeBytes is the exact byte size from the Hugging Face API.
	SizeBytes int64
	// Note is a short description for the UI.
	Note string
	// Recommended marks the default for a typical Windows 11 PC.
	Recommended bool
}

// FileName is the on-disk file name (the same as in the repository).
func (m ModelOption) FileName() string { return m.File }

// DownloadURL builds the download URL on a Hugging Face compatible host.
func (m ModelOption) DownloadURL(base string) string {
	base = strings.TrimSuffix(base, "/")
	return fmt.Sprintf("%s/%s/resolve/main/%s?download=true", base, m.Repo, m.File)
}

// SizeLabel renders the human-readable size, e.g. "1.06 GB".
func (m ModelOption) SizeLabel() string { return HumanSize(m.SizeBytes) }

// DefaultModelID is the model offered as the default on first run.
const DefaultModelID = "hy-mt2-1.8b-q4-k-m"

// Catalog is every model this app offers to download. File names and sizes
// come from the Hugging Face repositories (tencent/Hy-MT2-1.8B-GGUF and
// tencent/Hy-MT2-7B-GGUF). The 30B mixture-of-experts model stays out of the
// one-click list: its 4-bit file needs roughly 18 GB of RAM/VRAM.
var Catalog = []ModelOption{
	{"hy-mt2-1.8b-q4-k-m", "Hy-MT2-1.8B", "tencent/Hy-MT2-1.8B-GGUF", "Hy-MT2-1.8B-Q4_K_M.gguf", 1_133_080_448, "CPU에서도 빠릅니다. 대부분의 PC에 권장합니다.", true},
	{"hy-mt2-1.8b-q6-k", "Hy-MT2-1.8B", "tencent/Hy-MT2-1.8B-GGUF", "Hy-MT2-1.8B-Q6_K.gguf", 1_474_785_120, "조금 더 높은 품질, 메모리 여유가 있을 때.", false},
	{"hy-mt2-1.8b-q8-0", "Hy-MT2-1.8B", "tencent/Hy-MT2-1.8B-GGUF", "Hy-MT2-1.8B-Q8_0.gguf", 1_908_528_192, "거의 원본에 가까운 품질. RAM 4GB 이상 권장.", false},
	{"hy-mt2-7b-q4-k-m", "Hy-MT2-7B", "tencent/Hy-MT2-7B-GGUF", "Hy-MT2-7B-Q4_K_M.gguf", 4_624_648_896, "품질 우선. RAM 8GB 이상 또는 GPU 권장.", false},
	{"hy-mt2-7b-q6-k", "Hy-MT2-7B", "tencent/Hy-MT2-7B-GGUF", "HY-MT2-7B-Q6_K.gguf", 6_164_482_720, "7B 고품질. RAM 10GB 이상 권장.", false},
	{"hy-mt2-7b-q8-0", "Hy-MT2-7B", "tencent/Hy-MT2-7B-GGUF", "HY-MT2-7B-Q8_0.gguf", 7_981_928_896, "7B 최고 품질. RAM 12GB 이상 권장.", false},
}

// ModelByID looks a catalogue entry up by id.
func ModelByID(id string) (ModelOption, bool) {
	for _, option := range Catalog {
		if option.ID == id {
			return option, true
		}
	}
	return ModelOption{}, false
}

// RecommendedModel is the default catalogue entry.
func RecommendedModel() ModelOption {
	option, ok := ModelByID(DefaultModelID)
	if !ok {
		panic("default model exists")
	}
	return option
}

// IsModelFile reports whether the path looks like a GGUF model file.
func IsModelFile(name string) bool {
	return strings.EqualFold(filepath.Ext(name), ".gguf")
}

// HumanSize formats a byte count as B/MB/GB.
func HumanSize(bytes int64) string {
	const mb = 1024.0 * 1024.0
	const gb = mb * 1024.0
	f := float64(bytes)
	switch {
	case f >= gb:
		return fmt.Sprintf("%.2f GB", f/gb)
	case f >= mb:
		return fmt.Sprintf("%.0f MB", f/mb)
	default:
		return fmt.Sprintf("%d B", bytes)
	}
}

// PartPath is the temporary file used while downloading.
func PartPath(destination string) string {
	name := filepath.Base(destination)
	if name == "" || name == "." || name == "/" {
		name = "model.gguf"
	}
	return filepath.Join(filepath.Dir(destination), name+".part")
}

// CurlArgs builds the arguments for the bundled `curl.exe` downloader.
// Windows 11 ships curl.exe, which supports resuming (-C -), redirects and
// retries, so no TLS stack has to be linked for a one-time model fetch.
func CurlArgs(url, part string) []string {
	return []string{
		"-L", "--fail", "--retry", "3", "--retry-delay", "2",
		"--connect-timeout", "30", "--silent", "--show-error",
		"-C", "-", "-o", part, url,
	}
}

// DownloadProgress derives download progress from the size of the .part file.
type DownloadProgress struct {
	Downloaded int64 // bytes already written
	Total      int64 // expected size
}

// NewDownloadProgress builds a progress reading, clamping to the total.
func NewDownloadProgress(downloaded, total int64) DownloadProgress {
	if downloaded > total {
		downloaded = total
	}
	return DownloadProgress{Downloaded: downloaded, Total: total}
}

// Fraction in 0.0..=1.0.
func (p DownloadProgress) Fraction() float64 {
	if p.Total == 0 {
		return 0
	}
	f := float64(p.Downloaded) / float64(p.Total)
	if f > 1 {
		f = 1
	}
	return f
}

// Label renders "42% · 480 MB / 1.06 GB".
func (p DownloadProgress) Label() string {
	return fmt.Sprintf("%.0f%% · %s / %s", p.Fraction()*100, HumanSize(p.Downloaded), HumanSize(p.Total))
}
