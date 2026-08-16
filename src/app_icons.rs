use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

/// Abstract socket name the on-device server listens on.
const SOCKET_NAME: &str = "porpita";
/// Where the bundled DEX is pushed on the device.
const DEX_REMOTE_PATH: &str = "/data/local/tmp/porpita/porpita.dex";
/// Largest acceptable length-prefixed JSON frame.
const MAX_FRAME: usize = 10 * 1024 * 1024;
/// Per-request id counter.
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// The local folder where downloaded PNGs are cached per device:
/// `<app-data>/app_icons/<device-id>`.
fn icon_dir(serial: &str) -> PathBuf {
    crate::adb::app_data_dir().join("app_icons").join(serial)
}

/// Run `adb -s <serial> <args>` and return stdout, bailing on a non-zero exit.
fn run_adb(serial: &str, args: &[&str]) -> anyhow::Result<Vec<u8>> {
    let output = Command::new(crate::adb::adb_path())
        .arg("-s")
        .arg(serial)
        .args(args)
        .output()?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("adb {} failed: {}", args.join(" "), message);
    }
    Ok(output.stdout)
}

/// Whether the on-device server is already listening, checked via the abstract
/// socket showing up in `/proc/net/unix` as `@porpita`.
fn server_running(serial: &str) -> bool {
    run_adb(serial, &["shell", "cat", "/proc/net/unix"])
        .map(|output| String::from_utf8_lossy(&output).contains(&format!("@{SOCKET_NAME}")))
        .unwrap_or(false)
}

/// Write the bundled DEX to a temp file and push it to the device.
fn push_dex(serial: &str) -> anyhow::Result<()> {
    let temp = std::env::temp_dir().join("hakata-porpita.dex");
    std::fs::write(&temp, include_bytes!("../assets/porpita.dex"))?;
    let output = Command::new(crate::adb::adb_path())
        .arg("-s")
        .arg(serial)
        .arg("push")
        .arg(&temp)
        .arg(DEX_REMOTE_PATH)
        .output()?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("adb push failed: {}", message);
    }
    Ok(())
}

/// Launch the server on the device via `app_process`. The child adb client is
/// detached; the server keeps running on the device after this returns.
fn start_server(serial: &str) -> anyhow::Result<()> {
    let command = format!(
        "CLASSPATH={DEX_REMOTE_PATH} app_process /system/bin io.porpita.server.Server"
    );
    Command::new(crate::adb::adb_path())
        .arg("-s")
        .arg(serial)
        .arg("shell")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

/// Poll for the server socket for up to 10 seconds.
fn wait_for_server(serial: &str) -> anyhow::Result<()> {
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(200));
        if server_running(serial) {
            return Ok(());
        }
    }
    anyhow::bail!("porpita server did not start within 10 seconds");
}

/// Make sure the on-device server is up: push the DEX and launch it if the
/// socket is not already listening.
fn ensure_server_running(serial: &str) -> anyhow::Result<()> {
    if !server_running(serial) {
        push_dex(serial)?;
        start_server(serial)?;
        wait_for_server(serial)?;
    }
    Ok(())
}

/// Pick an unused TCP port on the host by binding to port 0.
fn find_free_port() -> anyhow::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// Forward a fresh local port to the device's abstract socket and open a
/// connection to it.
fn connect(serial: &str) -> anyhow::Result<TcpStream> {
    let port = find_free_port()?;
    run_adb(
        serial,
        &["forward", &format!("tcp:{port}"), &format!("localabstract:{SOCKET_NAME}")],
    )?;
    let stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    Ok(stream)
}

/// A length-prefixed JSON request frame: 4-byte big-endian length + JSON body.
fn encode_request(id: &str, method: &str, params: Option<&Value>) -> Vec<u8> {
    let request = match params {
        Some(params) => json!({"id": id, "method": method, "params": params}),
        None => json!({"id": id, "method": method}),
    };
    let payload = serde_json::to_vec(&request).expect("request serializes");
    let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&payload);
    frame
}

/// Read one length-prefixed JSON response frame and return its `result` field.
fn read_response<R: Read>(stream: &mut R) -> anyhow::Result<Value> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        anyhow::bail!("invalid response frame length {length}");
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    let response: Value = serde_json::from_slice(&body)?;
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

/// Send one request and wait for the matching response.
fn send_request(stream: &mut TcpStream, method: &str, params: Option<&Value>) -> anyhow::Result<Value> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let frame = encode_request(&format!("h{id}"), method, params);
    stream.write_all(&frame)?;
    stream.flush()?;
    read_response(stream)
}

/// Ask the device for each package's on-device icon path. A package maps to an
/// empty string when it has no icon resource.
fn get_app_icons(serial: &str, packages: &[String]) -> anyhow::Result<HashMap<String, String>> {
    let mut stream = connect(serial)?;
    let params = json!({"packageNames": packages});
    let result = send_request(&mut stream, "getAppIcons", Some(&params))?;
    let mut icons = HashMap::new();
    if let Some(object) = result.as_object() {
        for (package, path) in object {
            if let Some(path) = path.as_str() {
                icons.insert(package.clone(), path.to_string());
            }
        }
    }
    Ok(icons)
}

/// Start the device HTTP file server and forward it to a fresh local port.
/// Returns the local port to download from.
fn start_file_server(serial: &str) -> anyhow::Result<u16> {
    let mut stream = connect(serial)?;
    let result = send_request(&mut stream, "startFileServer", None)?;
    let device_port = result
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("file server did not report a port"))?
        as u16;
    let local_port = find_free_port()?;
    run_adb(
        serial,
        &[
            "forward",
            &format!("tcp:{local_port}"),
            &format!("tcp:{device_port}"),
        ],
    )?;
    Ok(local_port)
}

/// Download one icon PNG from the forwarded file server into `dest`.
fn download_icon(local_port: u16, device_path: &str, dest: &Path) -> anyhow::Result<()> {
    let url = format!("http://127.0.0.1:{local_port}{device_path}");
    let response = ureq::get(&url).call()?;
    if response.status() != 200 {
        anyhow::bail!("icon download returned {}", response.status());
    }
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest)?;
    std::io::copy(&mut reader, &mut file)?;
    Ok(())
}

/// Fetch icons for the given packages on `serial`, caching the PNGs under
/// `<app-data>/app_icons/<serial>/<package>.png`. Returns the packages that
/// produced a local icon file (a disk-cached file is reused without
/// re-downloading).
pub(crate) fn fetch_icons(serial: &str, packages: &[String]) -> anyhow::Result<HashMap<String, PathBuf>> {
    if packages.is_empty() {
        return Ok(HashMap::new());
    }
    ensure_server_running(serial)?;
    let icon_paths = get_app_icons(serial, packages)?;
    if icon_paths.is_empty() {
        return Ok(HashMap::new());
    }
    let local_port = start_file_server(serial)?;
    let dir = icon_dir(serial);
    std::fs::create_dir_all(&dir)?;
    let mut result = HashMap::new();
    for (package, device_path) in icon_paths {
        if device_path.is_empty() {
            continue;
        }
        let dest = dir.join(format!("{package}.png"));
        if dest.exists() && dest.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
            result.insert(package, dest);
            continue;
        }
        if download_icon(local_port, &device_path, &dest).is_ok() {
            result.insert(package, dest);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_frame_is_length_prefixed_json() {
        let params = json!({"packageNames": ["com.a", "com.b"]});
        let frame = encode_request("h1", "getAppIcons", Some(&params));
        let length = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(length, frame.len() - 4);
        let body: Value = serde_json::from_slice(&frame[4..]).unwrap();
        assert_eq!(body["id"], "h1");
        assert_eq!(body["method"], "getAppIcons");
        assert_eq!(body["params"]["packageNames"][0], "com.a");
    }

    #[test]
    fn request_frame_omits_params_when_none() {
        let frame = encode_request("h2", "startFileServer", None);
        let body: Value = serde_json::from_slice(&frame[4..]).unwrap();
        assert!(body.get("params").is_none());
    }

    #[test]
    fn response_parser_extracts_result() {
        let response = json!({"id": "h1", "result": {"port": 9001}});
        let payload = serde_json::to_vec(&response).unwrap();
        let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(&payload);
        let mut cursor = std::io::Cursor::new(frame);
        let result = read_response(&mut cursor).unwrap();
        assert_eq!(result["port"], 9001);
    }

    #[test]
    fn response_parser_rejects_oversized_frames() {
        let mut cursor = std::io::Cursor::new(vec![0xB0, 0, 0, 0]);
        assert!(read_response(&mut cursor).is_err());
    }

    #[test]
    fn icon_cache_dir_is_per_device() {
        assert_ne!(icon_dir("emulator-5554"), icon_dir("emulator-5556"));
        assert!(icon_dir("emulator-5554").ends_with("app_icons/emulator-5554"));
    }
}
