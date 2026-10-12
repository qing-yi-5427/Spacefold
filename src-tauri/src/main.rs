#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#[cfg(windows)]
mod portable_runtime;

fn main() {
    if std::env::var_os("SPACEFOLD_BOOT_LOG").is_some() {
        let path = std::env::var_os("SPACEFOLD_BOOT_LOG").unwrap();
        let _ = std::fs::write(&path, "main entered\n");
        std::panic::set_hook(Box::new(move |info| {
            use std::io::Write;
            if let Ok(mut log) = std::fs::OpenOptions::new().append(true).open(&path) {
                let _ = writeln!(log, "{info}");
            }
            eprintln!("{info}");
        }));
    }
    #[cfg(windows)]
    if let Err(error) = portable_runtime::initialise() {
        portable_runtime::show_startup_error(&error);
        std::process::exit(1);
    }
    spacefold_lib::run();
}
