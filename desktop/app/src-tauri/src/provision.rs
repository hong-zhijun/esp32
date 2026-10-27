//! USB provisioning: lists serial ports and writes the Wi-Fi settings and the
//! computer address to the board. Serial I/O blocks, so callers run these in `spawn_blocking`.
use serde::{Deserialize, Serialize};
use serde_json::json;
use serialport::{SerialPort, SerialPortInfo, SerialPortType};
use std::io::{ErrorKind, Write};
use std::net::Ipv4Addr;
use std::thread;
use std::time::{Duration, Instant};

const ESPRESSIF_VID: u16 = 0x303A;
const BAUD_RATE: u32 = 115_200;
/// The board needs this long after the port opens before it reads a line.
const SETTLE: Duration = Duration::from_millis(1500);
const REPLY_TIMEOUT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(100);
const MAX_SSID_BYTES: usize = 32;
const MAX_PASSWORD_BYTES: usize = 63;
/// Boot logs share the port; only the tail matters for finding the verdict.
const KEEP_TAIL: usize = 1024;
const TRIM_AFTER: usize = 4096;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub path: String,
    pub label: String,
    /// The board's own USB serial port (Espressif vendor id).
    pub espressif: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionRequest {
    pub port: String,
    pub host: String,
    pub ssid: String,
    pub password: String,
}

/// What the board needs to reach this computer after provisioning.
#[derive(Debug, Clone)]
pub struct Target {
    pub token: String,
    pub computer_id: String,
    pub server_port: u16,
}

pub fn list_ports() -> Vec<PortInfo> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(describe)
        .collect();
    ports.sort_by(|a, b| {
        b.espressif
            .cmp(&a.espressif)
            .then_with(|| a.path.cmp(&b.path))
    });
    ports
}

fn describe(port: SerialPortInfo) -> PortInfo {
    match port.port_type {
        SerialPortType::UsbPort(usb) => {
            let espressif = usb.vid == ESPRESSIF_VID;
            let label = usb
                .product
                .or(usb.manufacturer)
                .unwrap_or_else(|| port.port_name.clone());
            PortInfo {
                path: port.port_name,
                label,
                espressif,
            }
        }
        _ => PortInfo {
            label: port.port_name.clone(),
            path: port.port_name,
            espressif: false,
        },
    }
}

/// Checks a request before any port is opened. The messages are shown to the user as they are.
pub fn validate(
    request: &ProvisionRequest,
    addresses: &[Ipv4Addr],
    ports: &[PortInfo],
) -> Result<(), String> {
    let ssid_bytes = request.ssid.len();
    if ssid_bytes == 0 || ssid_bytes > MAX_SSID_BYTES {
        return Err("SSID 不正确".into());
    }
    if request.password.len() > MAX_PASSWORD_BYTES {
        return Err("密码过长".into());
    }
    let host_ok = request
        .host
        .parse::<Ipv4Addr>()
        .is_ok_and(|ip| addresses.contains(&ip));
    if !host_ok {
        return Err("请选择电脑局域网地址".into());
    }
    if !ports.iter().any(|port| port.path == request.port) {
        return Err("串口不存在".into());
    }
    Ok(())
}

/// Writes the configure line and waits for the board's verdict. Blocking.
pub fn provision(request: &ProvisionRequest, target: &Target) -> Result<(), String> {
    let deadline = Instant::now() + REPLY_TIMEOUT;
    let mut port = serialport::new(&request.port, BAUD_RATE)
        .timeout(POLL)
        .open()
        .map_err(|_| "无法打开串口，请关闭 Arduino 串口监视器".to_string())?;
    // The board's USB serial expects DTR and RTS, as the Node version's default open did.
    let _ = port.write_data_terminal_ready(true);
    let _ = port.write_request_to_send(true);
    thread::sleep(SETTLE);
    port.write_all(configure_line(request, target).as_bytes())
        .map_err(|_| "串口连接失败".to_string())?;
    let _ = port.flush();
    await_verdict(port.as_mut(), deadline)
}

fn configure_line(request: &ProvisionRequest, target: &Target) -> String {
    let message = json!({
        "type": "configure",
        "ssid": request.ssid,
        "password": request.password,
        "token": target.token,
        "id": target.computer_id,
        "host": request.host,
        "port": target.server_port,
    });
    format!("{message}\n")
}

/// `CONFIG_SAVED` means success, `CONFIG_INVALID` means the board rejected the values, and silence
/// until the deadline means the firmware is not answering.
fn await_verdict(port: &mut dyn SerialPort, deadline: Instant) -> Result<(), String> {
    let mut seen: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 256];
    while Instant::now() < deadline {
        match port.read(&mut chunk) {
            Ok(read) => {
                seen.extend_from_slice(&chunk[..read]);
                if contains(&seen, b"CONFIG_SAVED") {
                    return Ok(());
                }
                if contains(&seen, b"CONFIG_INVALID") {
                    return Err("板子拒绝了这组 Wi‑Fi 参数，请检查名称与密码".into());
                }
                keep_tail(&mut seen);
            }
            Err(error) if error.kind() == ErrorKind::TimedOut => {}
            Err(_) => return Err("串口连接失败".into()),
        }
    }
    Err("设备未确认，请确认固件已烧录、串口未被占用".into())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn keep_tail(seen: &mut Vec<u8>) {
    if seen.len() > TRIM_AFTER {
        let excess = seen.len() - KEEP_TAIL;
        seen.drain(..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(ssid: &str, password: &str, host: &str, port: &str) -> ProvisionRequest {
        ProvisionRequest {
            port: port.into(),
            host: host.into(),
            ssid: ssid.into(),
            password: password.into(),
        }
    }

    fn ports() -> Vec<PortInfo> {
        vec![PortInfo {
            path: "COM3".into(),
            label: "USB".into(),
            espressif: true,
        }]
    }

    fn lan() -> Vec<Ipv4Addr> {
        vec!["192.168.1.20".parse().unwrap()]
    }

    #[test]
    fn accepts_a_well_formed_request() {
        assert!(validate(
            &request("家里", "pass", "192.168.1.20", "COM3"),
            &lan(),
            &ports()
        )
        .is_ok());
    }

    #[test]
    fn ssid_limits_are_counted_in_bytes() {
        let thirty_two = "a".repeat(32);
        let thirty_three = "a".repeat(33);
        assert!(validate(
            &request(&thirty_two, "", "192.168.1.20", "COM3"),
            &lan(),
            &ports()
        )
        .is_ok());
        let err = validate(
            &request(&thirty_three, "", "192.168.1.20", "COM3"),
            &lan(),
            &ports(),
        )
        .unwrap_err();
        assert_eq!(err, "SSID 不正确");
        let chinese = "家".repeat(11); // 33 bytes
        assert!(validate(
            &request(&chinese, "", "192.168.1.20", "COM3"),
            &lan(),
            &ports()
        )
        .is_err());
        assert!(validate(&request("", "", "192.168.1.20", "COM3"), &lan(), &ports()).is_err());
    }

    #[test]
    fn password_is_limited_to_sixty_three_bytes() {
        let sixty_three = "p".repeat(63);
        let sixty_four = "p".repeat(64);
        assert!(validate(
            &request("x", &sixty_three, "192.168.1.20", "COM3"),
            &lan(),
            &ports()
        )
        .is_ok());
        let err = validate(
            &request("x", &sixty_four, "192.168.1.20", "COM3"),
            &lan(),
            &ports(),
        )
        .unwrap_err();
        assert_eq!(err, "密码过长");
    }

    #[test]
    fn host_must_be_a_lan_address_of_this_computer() {
        let err = validate(&request("x", "", "10.9.9.9", "COM3"), &lan(), &ports()).unwrap_err();
        assert_eq!(err, "请选择电脑局域网地址");
        assert!(validate(&request("x", "", "not-an-ip", "COM3"), &lan(), &ports()).is_err());
    }

    #[test]
    fn port_must_be_present() {
        let err =
            validate(&request("x", "", "192.168.1.20", "COM9"), &lan(), &ports()).unwrap_err();
        assert_eq!(err, "串口不存在");
    }

    #[test]
    fn configure_line_matches_the_protocol_order() {
        let target = Target {
            token: "tok".into(),
            computer_id: "abc123".into(),
            server_port: 8787,
        };
        let line = configure_line(&request("家", "pw", "192.168.1.20", "COM3"), &target);
        assert_eq!(
            line,
            "{\"type\":\"configure\",\"ssid\":\"家\",\"password\":\"pw\",\"token\":\"tok\",\"id\":\"abc123\",\"host\":\"192.168.1.20\",\"port\":8787}\n"
        );
    }

    #[test]
    fn tail_keeps_the_latest_bytes_only() {
        let mut seen = vec![b'x'; TRIM_AFTER + 10];
        seen.extend_from_slice(b"CONFIG_SAVED");
        keep_tail(&mut seen);
        assert!(seen.len() <= TRIM_AFTER);
        assert!(contains(&seen, b"CONFIG_SAVED"));
    }
}
