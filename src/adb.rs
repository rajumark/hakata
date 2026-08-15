use std::io::{Read, Write};
use std::path::{PathBuf, Component};

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
