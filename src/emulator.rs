use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// Candidate roots for the Android SDK, in priority order: explicit env vars,
/// then the per-platform default install location.
fn sdk_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in ["ANDROID_SDK_ROOT", "ANDROID_HOME"] {
        if let Ok(path) = std::env::var(var)
            && !path.is_empty()
        {
            roots.push(PathBuf::from(path));
        }
    }
    #[cfg(target_os = "macos")]
    roots.push(dirs::home_dir().unwrap_or_default().join("Library/Android/sdk"));
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_default();
        roots.push(local.join("Android/Sdk"));
    }
    #[cfg(target_os = "linux")]
    roots.push(dirs::home_dir().unwrap_or_default().join("Android/Sdk"));
    roots
}

fn emulator_executable() -> &'static str {
    if cfg!(target_os = "windows") {
        "emulator.exe"
    } else {
        "emulator"
    }
}

/// The Android SDK emulator binary, found via the SDK roots above and then
/// PATH.
pub fn emulator_binary_path() -> Option<PathBuf> {
    for root in sdk_roots() {
        let candidate = root.join("emulator").join(emulator_executable());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(emulator_executable());
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Names from `emulator -list-avds` stdout, one AVD per line.
fn parse_avd_list(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// The AVDs installed on this machine. Requires the Android SDK emulator
/// binary, which is also what boots them.
pub fn list_avds() -> anyhow::Result<Vec<String>> {
    let Some(binary) = emulator_binary_path() else {
        anyhow::bail!("Android SDK emulator binary not found");
    };
    let output = std::process::Command::new(&binary)
        .arg("-list-avds")
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "emulator -list-avds failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(parse_avd_list(&String::from_utf8_lossy(&output.stdout)))
}

/// Spawn `command` fully detached from Hakata: `setsid()` gives the child its
/// own session and process group (no controlling terminal, no inherited
/// SIGHUP), and stdio is redirected to /dev/null so nothing pins it to this
/// process. When Hakata quits the emulator is reparented to the system and
/// keeps running.
#[cfg(unix)]
fn spawn_detached(mut command: Command) -> std::io::Result<std::process::Child> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    command.spawn()
}

/// Launch an AVD. Spawns the emulator detached; it keeps running after this
/// returns and shows up in `adb devices` once it boots.
pub fn start_avd(name: &str) -> anyhow::Result<()> {
    let Some(binary) = emulator_binary_path() else {
        anyhow::bail!("Android SDK emulator binary not found");
    };
    let mut command = Command::new(&binary);
    command.arg("-avd").arg(name);
    #[cfg(unix)]
    spawn_detached(command)?;
    #[cfg(not(unix))]
    command.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_avd_names_one_per_line() {
        assert_eq!(
            parse_avd_list("Pixel_7\nNexus_5X\n"),
            vec!["Pixel_7".to_string(), "Nexus_5X".to_string()]
        );
    }

    #[test]
    fn trims_whitespace_and_ignores_blank_lines() {
        assert_eq!(
            parse_avd_list("  Pixel_7  \n\n   \nNexus_5X"),
            vec!["Pixel_7".to_string(), "Nexus_5X".to_string()]
        );
    }

    #[test]
    fn empty_output_is_empty() {
        assert!(parse_avd_list("").is_empty());
    }
}
