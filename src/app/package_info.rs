/// Basic facts extracted from `adb -s <device> shell dumpsys package <pkg>`
/// output. Only the fields the Apps Overview tab shows; the raw dump stays on
/// the entity so other tabs can parse what they need later.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PackageInfo {
    pub version_name: Option<String>,
    pub version_code: Option<String>,
    pub target_sdk: Option<String>,
    pub min_sdk: Option<String>,
    pub uid: Option<String>,
    pub first_install_time: Option<String>,
    pub last_update_time: Option<String>,
    pub data_dir: Option<String>,
    pub code_path: Option<String>,
    pub flags: Vec<String>,
}

/// Pull the requested fields out of a `dumpsys package` dump. Several keys
/// share a line (`versionCode=4100000 minSdk=26 targetSdk=34`), so each key
/// is looked up anywhere in the line and the first hit wins.
pub fn parse_package_info(dump: &str) -> PackageInfo {
    let mut info = PackageInfo::default();
    for line in dump.lines() {
        if let Some(rest) = value_after(line, "versionName=") {
            info.version_name = Some(rest.to_string());
        }
        if let Some(rest) = value_after(line, "versionCode=") {
            info.version_code = Some(rest.to_string());
        }
        if let Some(rest) = value_after(line, "targetSdk=") {
            info.target_sdk = Some(rest.to_string());
        }
        if let Some(rest) = value_after(line, "minSdk=") {
            info.min_sdk = Some(rest.to_string());
        }
        if info.uid.is_none()
            && let Some(rest) = value_after(line, "userId=")
        {
            info.uid = Some(rest.to_string());
        }
        if info.uid.is_none()
            && let Some(rest) = value_after(line, "uid=")
        {
            info.uid = Some(rest.to_string());
        }
        if let Some(rest) = remainder_after(line, "firstInstallTime=") {
            info.first_install_time = Some(rest.to_string());
        }
        if let Some(rest) = remainder_after(line, "lastUpdateTime=") {
            info.last_update_time = Some(rest.to_string());
        }
        if let Some(rest) = value_after(line, "dataDir=") {
            info.data_dir = Some(rest.to_string());
        }
        if let Some(rest) = value_after(line, "codePath=") {
            info.code_path = Some(rest.to_string());
        }
        if let Some(start) = line.find("flags=[") {
            let rest = &line[start + "flags=[".len()..];
            let end = rest.find(']').unwrap_or(rest.len());
            info.flags = rest[..end].split_whitespace().map(str::to_string).collect();
        }
    }
    info
}

/// The next whitespace-delimited token after `key` anywhere in `line`.
fn value_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.find(key).map(|start| {
        line[start + key.len()..]
            .split_whitespace()
            .next()
            .unwrap_or("")
    })
}

/// The rest of `line` after `key`, trimmed.
fn remainder_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.find(key).map(|start| line[start + key.len()..].trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"Package [com.example.app] (abcdef):
      userId=10123
      sharedUser=null
      pkg=Package{...}
      versionCode=4100000 minSdk=26 targetSdk=34
      versionName=4.1.0
      firstInstallTime=2024-01-15 09:30:00
      lastUpdateTime=2024-03-02 18:45:12
      dataDir=/data/user/0/com.example.app
      codePath=/data/app/~~x==/com.example.app/base.apk
      flags=[ DEBUGGABLE HAS_CODE ALLOW_CLEAR_USER_DATA ]
      primaryCpuAbi=arm64-v8a
    "#;

    #[test]
    fn parses_basic_fields() {
        let info = parse_package_info(SAMPLE);
        assert_eq!(info.version_name.as_deref(), Some("4.1.0"));
        assert_eq!(info.version_code.as_deref(), Some("4100000"));
        assert_eq!(info.target_sdk.as_deref(), Some("34"));
        assert_eq!(info.min_sdk.as_deref(), Some("26"));
        assert_eq!(info.uid.as_deref(), Some("10123"));
        assert_eq!(
            info.first_install_time.as_deref(),
            Some("2024-01-15 09:30:00")
        );
        assert_eq!(
            info.last_update_time.as_deref(),
            Some("2024-03-02 18:45:12")
        );
        assert_eq!(
            info.data_dir.as_deref(),
            Some("/data/user/0/com.example.app")
        );
        assert_eq!(
            info.code_path.as_deref(),
            Some("/data/app/~~x==/com.example.app/base.apk")
        );
        assert_eq!(
            info.flags,
            vec!["DEBUGGABLE", "HAS_CODE", "ALLOW_CLEAR_USER_DATA"]
        );
    }

    #[test]
    fn empty_dump_yields_defaults() {
        assert_eq!(parse_package_info(""), PackageInfo::default());
        assert_eq!(parse_package_info("no useful keys here"), PackageInfo::default());
    }

    #[test]
    fn picks_up_uid_when_no_user_id() {
        let info = parse_package_info("  uid=10001\n  versionName=1.0\n");
        assert_eq!(info.uid.as_deref(), Some("10001"));
        assert_eq!(info.version_name.as_deref(), Some("1.0"));
    }
}
