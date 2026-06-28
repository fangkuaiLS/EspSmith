#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // --mcp-server 模式（CodeWhale run 模式用）
    if args.iter().any(|arg| arg == "--mcp-server") {
        if let Err(err) = esp_smith_lib::run_mcp_stdio() {
            eprintln!("EspSmith MCP server failed: {err}");
            std::process::exit(1);
        }
        return;
    }

    // --version / --help: 纯 CLI，不启动 GUI
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("espsmith {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("EspSmith - AI-powered ESP32 development");
        println!("Usage: espsmith.exe <command> [options]");
        println!();
        println!("Commands:");
        println!("  build                 Build ESP-IDF project");
        println!("  flash                 Flash firmware to device");
        println!("  monitor               Monitor serial output");
        println!("  list-ports            List available serial ports");
        println!("  build-flash-monitor   Build, flash and monitor");
        println!("  get-targets           List supported ESP32 targets");
        println!("  disconnect            Disconnect serial ports");
        println!("  closed-loop           One-click JTAG/UART closed-loop: build→flash→verify");
        println!("  jtag-runtime-check    Deep JTAG runtime check with breakpoints & GDB");
        println!("  openocd-start         Start OpenOCD JTAG server");
        println!("  openocd-stop          Stop OpenOCD JTAG server");
        println!("  openocd-is-running    Check if OpenOCD is running");
        println!("  detect-connection     Detect JTAG vs UART connection mode");
        println!("  get-connection-mode   Get cached connection mode");
        println!("  get-hardware-config   Read .espsmith/hardware_config.json");
        println!("  get-idf-path          Get ESP-IDF path from project or environment");
        println!();
        println!("Options:");
        println!("  --project <path>      Open project in new GUI instance");
        println!("  --mcp-server          Run as MCP server over stdio");
        println!("  --version, -V         Show version");
        println!("  --help, -h            Show this help");
        return;
    }

    // CLI 子命令模式
    // Note: For CodeWhale's exec_shell, use espsmith-cli.exe (console subsystem)
    // instead of espsmith.exe (GUI subsystem), because GUI apps can't output to pipes.
    // This path is kept for backwards compatibility and direct CLI usage.
    let is_cli = args.iter()
        .skip(1)
        .any(|a| {
            !a.starts_with('-')
                && a.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c == '_')
                && a.len() >= 3
        });

    if is_cli {
        let _ = esp_smith_lib::run_cli();
        return;
    }

    // GUI 模式：解析 --project 参数（由 open_project_new_instance 传入）
    let startup_project = args.iter().enumerate().find_map(|(i, arg)| {
        if arg == "--project" {
            args.get(i + 1).cloned()
        } else if let Some(rest) = arg.strip_prefix("--project=") {
            Some(rest.to_string())
        } else {
            None
        }
    });
    esp_smith_lib::commands::project::set_startup_project(startup_project);

    // GUI 模式：启动时立即隐藏控制台窗口（避免 CodeWhale 子进程弹出黑框）
    // ShowWindow(SW_HIDE) 只隐藏窗口，不释放控制台，stdout/stderr 仍可正常输出
    #[cfg(windows)]
    hide_console_window();

    esp_smith_lib::run()
}

#[cfg(windows)]
fn hide_console_window() {
    const SW_HIDE: i32 = 0;
    extern "system" {
        fn GetConsoleWindow() -> isize;
        fn ShowWindow(hwnd: isize, nCmdShow: i32) -> i32;
    }
    unsafe {
        let hwnd = GetConsoleWindow();
        if hwnd != 0 {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}
