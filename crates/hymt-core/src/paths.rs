//! Where the application keeps its files.
//!
//! Two layouts are supported:
//!
//! * **Portable** -- if `portable.txt` sits next to the executable, or a `models` folder is
//!   next to it, everything stays beside the `.exe` (handy for a USB stick).
//! * **Installed** -- otherwise `%LOCALAPPDATA%\HyMT`, falling back to `~/.hymt` on machines
//!   without that variable (and to the temp folder as a last resort).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Resolved locations for settings, models and logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    /// Uses an explicit root directory.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Detects the layout for the running executable.
    pub fn detect() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let local_appdata = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        Self::detect_with(exe_dir.as_deref(), local_appdata.as_deref())
    }

    /// Pure detection used by tests and by [`AppPaths::detect`].
    pub fn detect_with(exe_dir: Option<&Path>, local_appdata: Option<&Path>) -> Self {
        if let Some(dir) = exe_dir {
            let portable_marker = dir.join("portable.txt").is_file();
            let portable_models = dir.join("models").is_dir();
            if portable_marker || portable_models {
                return Self::from_root(dir);
            }
        }
        if let Some(base) = local_appdata {
            return Self::from_root(base.join("HyMT"));
        }
        for key in ["USERPROFILE", "HOME"] {
            if let Some(home) = std::env::var_os(key) {
                return Self::from_root(PathBuf::from(home).join(".hymt"));
            }
        }
        Self::from_root(std::env::temp_dir().join("hymt"))
    }

    /// Base directory for everything else.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `settings.json`
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Folder holding `.gguf` models.
    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    /// Folder where a `llama-server` binary can be placed.
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }

    /// Folder for engine logs.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Log file of the managed llama.cpp server.
    pub fn engine_log(&self) -> PathBuf {
        self.logs_dir().join("llama-server.log")
    }

    /// Log file of the application itself.
    pub fn app_log(&self) -> PathBuf {
        self.logs_dir().join("hymt.log")
    }

    /// Creates the directory tree. Safe to call repeatedly.
    pub fn ensure(&self) -> io::Result<()> {
        for dir in [
            self.root.clone(),
            self.models_dir(),
            self.runtime_dir(),
            self.logs_dir(),
        ] {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn portable_mode_is_used_when_the_marker_exists() {
        let dir = TempDir::new("paths-portable");
        fs::write(dir.path().join("portable.txt"), "").unwrap();
        let resolved = AppPaths::detect_with(Some(dir.path()), Some(Path::new("C:/AppData")));
        assert_eq!(resolved.root(), dir.path());
    }

    #[test]
    fn a_local_models_folder_also_means_portable() {
        let dir = TempDir::new("paths-portable-models");
        fs::create_dir_all(dir.path().join("models")).unwrap();
        let resolved = AppPaths::detect_with(Some(dir.path()), Some(Path::new("C:/AppData")));
        assert_eq!(resolved.root(), dir.path());
    }

    #[test]
    fn installed_layout_uses_local_appdata() {
        let dir = TempDir::new("paths-installed");
        let resolved = AppPaths::detect_with(Some(dir.path()), Some(Path::new("C:/AppData")));
        assert_eq!(resolved.root(), Path::new("C:/AppData").join("HyMT"));
        assert!(resolved.settings_file().ends_with("settings.json"));
        assert!(resolved.models_dir().ends_with("models"));
    }

    #[test]
    fn ensure_creates_every_directory() {
        let dir = TempDir::new("paths-ensure");
        let paths = AppPaths::from_root(dir.path().join("root"));
        paths.ensure().unwrap();
        assert!(paths.models_dir().is_dir());
        assert!(paths.runtime_dir().is_dir());
        assert!(paths.logs_dir().is_dir());
    }
}
