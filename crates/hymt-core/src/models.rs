//! The official Tencent GGUF catalogue and download helpers.
//!
//! File names and sizes are taken from the Hugging Face repositories
//! (`tencent/Hy-MT2-1.8B-GGUF`, `tencent/Hy-MT2-7B-GGUF`) and are used to show an accurate
//! size before downloading and to verify a completed download.
//!
//! The 30B-A3B mixture-of-experts model is supported too -- the app scans the models folder,
//! so any GGUF placed there is usable -- but it is not offered as a one-click download because
//! its 4-bit file needs roughly 18 GB of RAM/VRAM.

use std::path::{Path, PathBuf};

/// One downloadable model file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelOption {
    /// Stable identifier used by the UI.
    pub id: &'static str,
    /// Model family, e.g. `Hy-MT2-1.8B`.
    pub family: &'static str,
    /// Hugging Face repository.
    pub repo: &'static str,
    /// Exact file name inside the repository.
    pub file: &'static str,
    /// Exact byte size from the Hugging Face API.
    pub size_bytes: u64,
    /// Short description for the UI.
    pub note: &'static str,
    /// Whether this is the recommended default for a typical Windows 11 PC.
    pub recommended: bool,
}

impl ModelOption {
    /// File name on disk (the same as in the repository).
    pub fn file_name(&self) -> &'static str {
        self.file
    }

    /// Download URL on a Hugging Face compatible host.
    pub fn download_url(&self, base: &str) -> String {
        format!(
            "{}/{}/resolve/main/{}?download=true",
            base.trim_end_matches('/'),
            self.repo,
            self.file
        )
    }

    /// Human-readable size, e.g. `1.06 GB`.
    pub fn size_label(&self) -> String {
        human_size(self.size_bytes)
    }
}

/// Model offered as the default on first run.
pub const DEFAULT_MODEL_ID: &str = "hy-mt2-1.8b-q4-k-m";

/// Every model this app offers to download.
pub const CATALOG: &[ModelOption] = &[
    ModelOption {
        id: "hy-mt2-1.8b-q4-k-m",
        family: "Hy-MT2-1.8B",
        repo: "tencent/Hy-MT2-1.8B-GGUF",
        file: "Hy-MT2-1.8B-Q4_K_M.gguf",
        size_bytes: 1_133_080_448,
        note: "CPU에서도 빠릅니다. 대부분의 PC에 권장합니다.",
        recommended: true,
    },
    ModelOption {
        id: "hy-mt2-1.8b-q6-k",
        family: "Hy-MT2-1.8B",
        repo: "tencent/Hy-MT2-1.8B-GGUF",
        file: "Hy-MT2-1.8B-Q6_K.gguf",
        size_bytes: 1_474_785_120,
        note: "조금 더 높은 품질, 메모리 여유가 있을 때.",
        recommended: false,
    },
    ModelOption {
        id: "hy-mt2-1.8b-q8-0",
        family: "Hy-MT2-1.8B",
        repo: "tencent/Hy-MT2-1.8B-GGUF",
        file: "Hy-MT2-1.8B-Q8_0.gguf",
        size_bytes: 1_908_528_192,
        note: "거의 원본에 가까운 품질. RAM 4GB 이상 권장.",
        recommended: false,
    },
    ModelOption {
        id: "hy-mt2-7b-q4-k-m",
        family: "Hy-MT2-7B",
        repo: "tencent/Hy-MT2-7B-GGUF",
        file: "Hy-MT2-7B-Q4_K_M.gguf",
        size_bytes: 4_624_648_896,
        note: "품질 우선. RAM 8GB 이상 또는 GPU 권장.",
        recommended: false,
    },
    ModelOption {
        id: "hy-mt2-7b-q6-k",
        family: "Hy-MT2-7B",
        repo: "tencent/Hy-MT2-7B-GGUF",
        file: "HY-MT2-7B-Q6_K.gguf",
        size_bytes: 6_164_482_720,
        note: "7B 고품질. RAM 10GB 이상 권장.",
        recommended: false,
    },
    ModelOption {
        id: "hy-mt2-7b-q8-0",
        family: "Hy-MT2-7B",
        repo: "tencent/Hy-MT2-7B-GGUF",
        file: "HY-MT2-7B-Q8_0.gguf",
        size_bytes: 7_981_928_896,
        note: "7B 최고 품질. RAM 12GB 이상 권장.",
        recommended: false,
    },
];

/// Looks a catalogue entry up by id.
pub fn by_id(id: &str) -> Option<&'static ModelOption> {
    CATALOG.iter().find(|option| option.id == id)
}

/// The default catalogue entry.
pub fn recommended() -> &'static ModelOption {
    by_id(DEFAULT_MODEL_ID).expect("default model exists")
}

/// Returns true when the path looks like a GGUF model file.
pub fn is_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
}

/// Formats a byte count as `MB`/`GB`.
pub fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GB {
        format!("{:.2} GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.0} MB", bytes_f / MB)
    } else {
        format!("{bytes} B")
    }
}

/// Temporary file used while downloading.
pub fn part_path(destination: &Path) -> PathBuf {
    let mut name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model.gguf".to_string());
    name.push_str(".part");
    destination.with_file_name(name)
}

/// Arguments for the bundled `curl.exe` downloader.
///
/// Windows 11 ships `curl.exe`, which supports resuming (`-C -`), redirects and retries -- so
/// the application does not need to link a TLS stack just to fetch a model once.
pub fn curl_args(url: &str, part: &Path) -> Vec<String> {
    vec![
        "-L".into(),
        "--fail".into(),
        "--retry".into(),
        "3".into(),
        "--retry-delay".into(),
        "2".into(),
        "--connect-timeout".into(),
        "30".into(),
        "--silent".into(),
        "--show-error".into(),
        "-C".into(),
        "-".into(),
        "-o".into(),
        part.to_string_lossy().into_owned(),
        url.into(),
    ]
}

/// Download progress derived from the size of the `.part` file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DownloadProgress {
    /// Bytes already written.
    pub downloaded: u64,
    /// Expected total size.
    pub total: u64,
}

impl DownloadProgress {
    /// Builds a progress reading, clamping to the expected total.
    pub fn new(downloaded: u64, total: u64) -> Self {
        Self {
            downloaded: downloaded.min(total),
            total,
        }
    }

    /// Fraction in `0.0..=1.0`.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.downloaded as f32 / self.total as f32).clamp(0.0, 1.0)
    }

    /// `42% · 480 MB / 1.06 GB`
    pub fn label(&self) -> String {
        format!(
            "{:.0}% · {} / {}",
            self.fraction() * 100.0,
            human_size(self.downloaded),
            human_size(self.total)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_entries_are_consistent() {
        assert!(by_id(DEFAULT_MODEL_ID).is_some());
        assert!(recommended().recommended);
        for option in CATALOG {
            assert!(option.file.ends_with(".gguf"), "{}", option.file);
            assert!(option.size_bytes > 100_000_000);
            assert!(option
                .download_url("https://huggingface.co")
                .contains(option.repo));
            assert!(!option.note.is_empty());
        }
    }

    #[test]
    fn builds_download_urls_for_hugging_face_and_mirrors() {
        let option = recommended();
        assert_eq!(
            option.download_url("https://huggingface.co/"),
            "https://huggingface.co/tencent/Hy-MT2-1.8B-GGUF/resolve/main/Hy-MT2-1.8B-Q4_K_M.gguf?download=true"
        );
        assert_eq!(
            option.download_url("https://hf-mirror.com"),
            "https://hf-mirror.com/tencent/Hy-MT2-1.8B-GGUF/resolve/main/Hy-MT2-1.8B-Q4_K_M.gguf?download=true"
        );
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(human_size(1_133_080_448), "1.06 GB");
        assert_eq!(human_size(5 * 1024 * 1024), "5 MB");
        assert_eq!(human_size(999), "999 B");
    }

    #[test]
    fn curl_args_resume_and_verify() {
        let args = curl_args(
            "https://example.com/model.gguf",
            Path::new("C:/models/model.gguf.part"),
        );
        assert!(args.windows(2).any(|pair| pair == ["-C", "-"]));
        assert!(args.contains(&"--fail".to_string()));
        assert_eq!(args.last().unwrap(), "https://example.com/model.gguf");
        assert_eq!(
            args[args.iter().position(|a| a == "-o").unwrap() + 1],
            "C:/models/model.gguf.part"
        );
    }

    #[test]
    fn part_path_keeps_the_model_name() {
        let part = part_path(Path::new("/tmp/Hy-MT2-1.8B-Q4_K_M.gguf"));
        assert_eq!(part, PathBuf::from("/tmp/Hy-MT2-1.8B-Q4_K_M.gguf.part"));
    }

    #[test]
    fn progress_is_clamped_and_readable() {
        let progress = DownloadProgress::new(512, 1024);
        assert!((progress.fraction() - 0.5).abs() < f32::EPSILON);
        assert!(progress.label().contains("50%"));
        let over = DownloadProgress::new(4096, 1024);
        assert_eq!(over.fraction(), 1.0);
        assert_eq!(DownloadProgress::new(0, 0).fraction(), 0.0);
    }

    #[test]
    fn recognizes_gguf_files_case_insensitively() {
        assert!(is_model_file(Path::new("a.GGUF")));
        assert!(!is_model_file(Path::new("a.part")));
    }
}
