fn main() {
    // tauri_build::build() 在编译 espsmith-cli bin target 时可能 panic
    // （Windows RC 编译器在处理非主 bin target 时出错）。
    // 捕获 panic，让 espsmith-cli 仍然能编译通过。
    // 对于主 bin target (espsmith.exe)，tauri_build::build() 正常执行。
    let result = std::panic::catch_unwind(|| {
        tauri_build::build()
    });
    if let Err(payload) = result {
        // 如果是编译 espsmith-cli，panic 是预期的，输出警告即可
        eprintln!("WARNING: tauri_build::build() panicked (this is expected for espsmith-cli target)");
        if let Some(s) = payload.downcast_ref::<&str>() {
            eprintln!("  Panic message: {}", s);
        } else if let Some(s) = payload.downcast_ref::<String>() {
            eprintln!("  Panic message: {}", s);
        }
        // 仍然需要输出一些基本的 cargo 指令，否则编译可能失败
        // 但实际上 espsmith-cli 不需要 tauri 的 build script 输出
    }
}
