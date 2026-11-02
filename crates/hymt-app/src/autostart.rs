//! Windows sign-in startup through the current user's `Run` key.
//!
//! The entry is per user (`HKEY_CURRENT_USER`) and points at the running executable, so it
//! needs no administrator rights and follows portable installations. `--autostart` marks a
//! launch that came from the registry; the app uses it to start in the tray when that option
//! is enabled.

use std::io;
use std::path::Path;

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "HyMT";

/// Adds or removes the current executable from the user's startup entries.
pub fn set_enabled(enabled: bool) -> io::Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    if enabled {
        let exe = std::env::current_exe()?;
        key.set_value(VALUE_NAME, &command_for(&exe))?;
    } else {
        match key.delete_value(VALUE_NAME) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

/// Command line registered for the current executable.
fn command_for(exe: &Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quotes_the_executable_and_marks_autostart() {
        let command = command_for(Path::new(r"C:\Program Files\HyMT\hymt.exe"));
        assert_eq!(command, r#""C:\Program Files\HyMT\hymt.exe" --autostart"#);
    }
}
