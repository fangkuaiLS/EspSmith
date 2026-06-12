//! Tauri 命令模块
//!
//! 所有后端命令按功能模块分离，便于维护和二次开发

pub mod project;
pub mod filesystem;
pub mod hardware;
pub mod build;
pub mod serial;
pub mod debug;
pub mod gdb_session;
pub mod openocd;
pub mod git_cmd;
