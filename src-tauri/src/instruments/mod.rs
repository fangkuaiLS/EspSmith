#![allow(dead_code)] // 诊断框架预留，整个模块尚未接入
//! Instrument abstraction — inspired by AEL's instruments/ module.
//!
//! Defines the Instrument trait and a simple registry pattern.
//! Each instrument represents a hardware tool (e.g., ESP32JTAG, ST-Link, DAPLink).

use serde::{Deserialize, Serialize};

/// Health status of an instrument.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded(String),
    Unhealthy(String),
    Unknown,
}

/// Result of an instrument health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub instrument_id: String,
    pub status: HealthStatus,
    /// Subsystem checks
    pub checks: Vec<SubsystemCheck>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsystemCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// The Instrument trait — any hardware tool that can run a health check.
pub trait Instrument: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn family(&self) -> InstrumentFamily;

    /// Run self-test: check network, GDB, capture subsystem, etc.
    fn run_health_check(&self) -> HealthReport;
}

/// Instrument family (probe + target).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstrumentFamily {
    ESP32JTAG,
    STLink,
    DAPLink,
    WCHLink,
    JLink,
    OpenOCDGeneric,
    Custom(String),
}

/// Simple instrument registry.
pub struct InstrumentRegistry {
    instruments: Vec<Box<dyn Instrument>>,
}

impl InstrumentRegistry {
    pub fn new() -> Self {
        Self { instruments: Vec::new() }
    }

    pub fn register(&mut self, instrument: Box<dyn Instrument>) {
        self.instruments.push(instrument);
    }

    pub fn list(&self) -> Vec<InstrumentInfo> {
        self.instruments
            .iter()
            .map(|i| InstrumentInfo {
                id: i.id().to_string(),
                name: i.name().to_string(),
                family: i.family().clone(),
                description: i.description().to_string(),
            })
            .collect()
    }

    pub fn find(&self, id: &str) -> Option<&dyn Instrument> {
        self.instruments
            .iter()
            .find(|i| i.id() == id)
            .map(|i| i.as_ref())
    }

    pub fn run_health_check(&self, id: &str) -> Option<HealthReport> {
        self.find(id).map(|i| i.run_health_check())
    }

    pub fn run_all_health_checks(&self) -> Vec<HealthReport> {
        self.instruments.iter().map(|i| i.run_health_check()).collect()
    }
}

impl Default for InstrumentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Lightweight info about a registered instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentInfo {
    pub id: String,
    pub name: String,
    pub family: InstrumentFamily,
    pub description: String,
}

pub mod doctor;