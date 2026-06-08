use std::process::Command;

fn main() {
    // Test output() directly
    match Command::new("cmd").arg("/C").arg("echo hello").output() {
        Ok(o) => println!("output OK: status={}, stdout={}", o.status, String::from_utf8_lossy(&o.stdout)),
        Err(e) => eprintln!("output ERR: {:?}", e),
    }
}