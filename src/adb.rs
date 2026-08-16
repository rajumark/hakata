use std::io::{Read, Write};
use std::path::{Component, PathBuf};

const DOWNLOAD_BASE: &str = "https://raw.githubusercontent.com/rajumark/adbcontent/main/";

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

/// One row of `adb devices` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbDevice {
    pub serial: String,
    pub state: String,
}

/// Parse `adb devices` output into attached devices.
///
/// Tolerates the header, blank lines, and daemon-startup chatter; only
/// lines of `serial<TAB>state` form are kept.
pub fn parse_adb_devices(output: &str) -> Vec<AdbDevice> {
    let mut devices = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() || line == "List of devices attached" || line.starts_with('*') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(serial), Some(state)) = (parts.next(), parts.next()) else {
            continue;
        };
        devices.push(AdbDevice {
            serial: serial.to_string(),
            state: state.to_string(),
        });
    }
    devices
}

/// Pick the default device after a refresh: keep the current selection while
/// it is still attached and ready, otherwise fall back to the first ready
/// device, otherwise none.
pub fn resolve_default_device(current: Option<&str>, ready: &[&str]) -> Option<String> {
    if let Some(current) = current {
        if ready.contains(&current) {
            return Some(current.to_string());
        }
    }
    ready.first().map(|serial| (*serial).to_string())
}

/// Parse `adb shell pm list packages` output (`package:com.example.app`
/// lines) into sorted, de-duplicated package names.
pub fn parse_packages(output: &str) -> Vec<String> {
    let mut packages: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("package:").map(str::trim))
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    packages.sort();
    packages.dedup();
    packages
}

fn platform_zip_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "platform-tools-macos.zip"
    } else if cfg!(target_os = "windows") {
        "platform-tools-windows.zip"
    } else {
        "platform-tools-linux.zip"
    }
}

/// Download the platform-tools zip for the current OS, extract it into the
/// app-data platform-tools dir, and mark the adb binary executable.
///
/// `progress` is called on the calling thread with a 0..=1 fraction as the
/// download advances. It runs off the UI thread: keep it non-blocking.
pub fn download_and_install(mut progress: impl FnMut(f32) + Send + 'static) -> anyhow::Result<()> {
    let dir = platform_tools_dir();
    std::fs::create_dir_all(&dir)?;

    let response = ureq::get(&format!("{DOWNLOAD_BASE}{}", platform_zip_name())).call()?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);

    let temp_zip = dir.join(".adb-bootstrap.zip");
    let mut downloaded = 0u64;
    {
        let mut file = std::fs::File::create(&temp_zip)?;
        let mut reader = response.into_reader();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            downloaded += read as u64;
            if total > 0 {
                progress((downloaded as f32 / total as f32).min(1.0));
            }
        }
    }

    let zip_file = std::fs::File::open(&temp_zip)?;
    let mut archive = zip::ZipArchive::new(zip_file)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let entry_name = match entry.enclosed_name() {
            Some(name) => name,
            None => continue,
        };

        let mut components = entry_name.components();
        let top = components.next();
        let under_platform_tools = matches!(
            top,
            Some(Component::Normal(name)) if name == "platform-tools"
        );
        if !under_platform_tools {
            continue;
        }
        let relative = components.as_path();
        if relative.as_os_str().is_empty() {
            continue;
        }

        let dest = dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut out)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    std::fs::remove_file(&temp_zip)?;

    if !is_installed() {
        anyhow::bail!(
            "download finished but adb is missing at {}",
            adb_path().display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ready_devices() {
        let devices =
            parse_adb_devices("List of devices attached\nemulator-5554\tdevice\nR58M123\tdevice\n");
        assert_eq!(
            devices,
            vec![
                AdbDevice {
                    serial: "emulator-5554".into(),
                    state: "device".into(),
                },
                AdbDevice {
                    serial: "R58M123".into(),
                    state: "device".into(),
                },
            ]
        );
    }

    #[test]
    fn empty_list_is_empty() {
        assert!(parse_adb_devices("List of devices attached\n\n").is_empty());
        assert!(parse_adb_devices("").is_empty());
    }

    #[test]
    fn skips_daemon_startup_chatter() {
        let output = "* daemon not running; starting now at tcp:5037\n\
            * daemon started successfully\n\
            List of devices attached\n\
            emulator-5554\tdevice\n";
        let devices = parse_adb_devices(output);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial, "emulator-5554");
    }

    #[test]
    fn keeps_unready_states() {
        let devices =
            parse_adb_devices("List of devices attached\nXYZ\tunauthorized\nABC\toffline\n");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].state, "unauthorized");
        assert_eq!(devices[1].state, "offline");
    }

    #[test]
    fn default_device_picks_first_ready_when_none_selected() {
        let ready = ["emulator-5554", "emulator-5556"];
        assert_eq!(
            resolve_default_device(None, &ready),
            Some("emulator-5554".to_string())
        );
    }

    #[test]
    fn default_device_keeps_current_selection() {
        let ready = ["emulator-5554", "emulator-5556"];
        assert_eq!(
            resolve_default_device(Some("emulator-5556"), &ready),
            Some("emulator-5556".to_string())
        );
    }

    #[test]
    fn default_device_falls_back_when_selection_removed() {
        let ready = ["emulator-5556"];
        assert_eq!(
            resolve_default_device(Some("emulator-5554"), &ready),
            Some("emulator-5556".to_string())
        );
    }

    #[test]
    fn default_device_is_none_without_ready_devices() {
        assert_eq!(resolve_default_device(Some("emulator-5554"), &[]), None);
        assert_eq!(resolve_default_device(None, &[]), None);
    }

    #[test]
    fn parses_package_lines_into_sorted_unique_names() {
        let output = "package:com.example.beta\npackage:com.example.alpha\npackage:com.example.alpha\n";
        assert_eq!(
            parse_packages(output),
            vec!["com.example.alpha", "com.example.beta"]
        );
    }

    #[test]
    fn ignores_non_package_lines() {
        let output = "List of devices attached\npackage:com.example.app\n\nerror: no devices\n";
        assert_eq!(parse_packages(output), vec!["com.example.app"]);
    }
}
