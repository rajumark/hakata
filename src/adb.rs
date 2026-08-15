use std::path::PathBuf;

/// Best-practice app-data root for the current platform:
/// macOS  ~/Library/Application Support/Hakata
/// Windows  %APPDATA%\Hakata
/// Linux  ~/.local/share/Hakata
pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Hakata")
}

/// The platform-tools folder inside the app-data dir, where adb lives.
pub fn platform_tools_dir() -> PathBuf {
    app_data_dir().join("platform-tools")
}

/// The adb binary itself. The platform-tools zips put it in the folder root
/// as `adb` (macOS/Linux) or `adb.exe` (Windows).
pub fn adb_path() -> PathBuf {
    let executable = if cfg!(target_os = "windows") {
        "adb.exe"
    } else {
        "adb"
    };
    platform_tools_dir().join(executable)
}

pub fn is_installed() -> bool {
    adb_path().is_file()
}
