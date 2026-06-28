//! Instrument doctor — health check summary for all instruments.
//!
//! Inspired by AEL's instrument_doctor.py.

use super::*;
use std::net::TcpStream;
use std::time::Duration;

/// Quick TCP reachability check.
pub fn check_tcp_reachable(host: &str, port: u16, timeout_ms: u64) -> SubsystemCheck {
    let addr_str = format!("{host}:{port}");
    let addr: std::net::SocketAddr = match addr_str.parse() {
        Ok(a) => a,
        Err(e) => return SubsystemCheck {
            name: format!("TCP {host}:{port}"),
            passed: false,
            message: format!("Invalid address '{}': {}", addr_str, e),
        },
    };
    match TcpStream::connect_timeout(
        &addr,
        Duration::from_millis(timeout_ms),
    ) {
        Ok(_) => SubsystemCheck {
            name: format!("TCP {host}:{port}"),
            passed: true,
            message: "Reachable".into(),
        },
        Err(e) => SubsystemCheck {
            name: format!("TCP {host}:{port}"),
            passed: false,
            message: format!("Unreachable: {e}"),
        },
    }
}

/// Check if a serial port is available.
pub fn check_serial_port(port: &str) -> SubsystemCheck {
    match serialport::available_ports() {
        Ok(ports) => {
            let found = ports.iter().any(|p| p.port_name == port);
            SubsystemCheck {
                name: format!("Serial {port}"),
                passed: found,
                message: if found {
                    "Available".into()
                } else {
                    "Not found".into()
                },
            }
        }
        Err(e) => SubsystemCheck {
            name: format!("Serial {port}"),
            passed: false,
            message: format!("Cannot enumerate: {e}"),
        },
    }
}

/// Summarize all health checks into a single status.
pub fn summarize(checks: &[SubsystemCheck]) -> HealthStatus {
    if checks.is_empty() {
        return HealthStatus::Unknown;
    }
    let passed = checks.iter().filter(|c| c.passed).count();
    let total = checks.len();
    let failed: Vec<&str> = checks
        .iter()
        .filter(|c| !c.passed)
        .map(|c| c.name.as_str())
        .collect();

    if passed == total {
        HealthStatus::Healthy
    } else if passed == 0 {
        HealthStatus::Unhealthy(format!("All {} checks failed: {}", failed.len(), failed.join(", ")))
    } else {
        HealthStatus::Degraded(format!(
            "{}/{} checks failed: {}",
            total - passed,
            total,
            failed.join(", ")
        ))
    }
}