// Console subsystem CLI wrapper for espsmith.
//
// The main espsmith.exe uses `windows_subsystem = "windows"` (GUI mode),
// which means the C runtime doesn't initialize stdout/stderr FILE* objects.
// When CodeWhale's exec_shell runs `espsmith.exe build` via PowerShell,
// PowerShell cannot capture stdout from a GUI subsystem process, resulting
// in "(no output)".
//
// This binary is a console subsystem program (default) that simply calls
// the same `run_cli()` function. Since it's a console app, PowerShell and
// pipes can capture its stdout/stderr correctly.

fn main() {
    let result = esp_smith_lib::run_cli();
    // Force flush stdout/stderr before exiting, and use process::exit to ensure
    // the process terminates immediately even if background threads or TCP
    // connections (IPC delegate) are still cleaning up. Without this, the AI's
    // bash tool may hang waiting for the process to exit.
    let code = if result.is_ok() { 0 } else { 1 };
    std::process::exit(code);
}
