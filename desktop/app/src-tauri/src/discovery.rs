//! mDNS advertisement (`_aipanel._tcp`) so the board can find this computer on the LAN without a
//! fixed address. The advertisement follows the LAN addresses as they change.
use crate::net;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use panel_core::debug_log::DebugLog;
use serde_json::json;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

const SERVICE_TYPE: &str = "_aipanel._tcp.local.";
const REFRESH: Duration = Duration::from_secs(30);

/// Keeps the advertisement in step with the computer's LAN addresses for the life of the app.
pub fn spawn(computer_id: String, port: u16, log: DebugLog) {
    tauri::async_runtime::spawn(async move {
        let daemon = match ServiceDaemon::new() {
            Ok(daemon) => daemon,
            Err(error) => {
                report(&log, &format!("mDNS 启动失败：{error}"));
                return;
            }
        };
        let mut advert = Advert::default();
        let mut ticker = tokio::time::interval(REFRESH);
        loop {
            ticker.tick().await;
            advert.follow(&daemon, &net::lan_ipv4(), &computer_id, port, &log);
        }
    });
}

#[derive(Default)]
struct Advert {
    /// Addresses the service is currently advertised on; empty when nothing is advertised.
    addresses: Vec<Ipv4Addr>,
    fullname: Option<String>,
    last_error: Option<String>,
}

impl Advert {
    fn follow(
        &mut self,
        daemon: &ServiceDaemon,
        addresses: &[Ipv4Addr],
        computer_id: &str,
        port: u16,
        log: &DebugLog,
    ) {
        if self.addresses == addresses {
            return;
        }
        if let Some(fullname) = self.fullname.take() {
            let _ = daemon.unregister(&fullname);
        }
        self.addresses.clear();
        if addresses.is_empty() {
            return;
        }
        match register(daemon, computer_id, port, addresses) {
            Ok(fullname) => {
                self.fullname = Some(fullname);
                self.addresses = addresses.to_vec();
                self.last_error = None;
            }
            Err(error) => {
                if self.last_error.as_deref() != Some(error.as_str()) {
                    report(log, &format!("mDNS 广播失败：{error}"));
                    self.last_error = Some(error);
                }
            }
        }
    }
}

fn register(
    daemon: &ServiceDaemon,
    computer_id: &str,
    port: u16,
    addresses: &[Ipv4Addr],
) -> Result<String, String> {
    let instance = format!("ai-panel-{}", &computer_id[..6]);
    let host = format!("{instance}.local.");
    let ips: Vec<IpAddr> = addresses.iter().map(|ip| IpAddr::V4(*ip)).collect();
    let properties = HashMap::from([
        ("id".to_string(), computer_id.to_string()),
        ("v".to_string(), "1".to_string()),
    ]);
    let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, &ips[..], port, properties)
        .map_err(|error| error.to_string())?;
    let fullname = info.get_fullname().to_string();
    daemon.register(info).map_err(|error| error.to_string())?;
    Ok(fullname)
}

fn report(log: &DebugLog, message: &str) {
    log.add(json!({"event": "mdns-error", "message": message}));
}
