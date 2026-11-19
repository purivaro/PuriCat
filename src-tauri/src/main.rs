// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started by an AI CLI as its MCP server: relay stdin and stdout to the running app's
    // socket and do nothing else. No window, no settings, no credentials.
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new("--mcp-bridge")) {
        let outcome = match args.next() {
            Some(socket) => puricat_core::mcp::bridge(std::path::Path::new(&socket)),
            None => Err(std::io::Error::other(
                "--mcp-bridge needs the socket's path",
            )),
        };
        if let Err(e) = outcome {
            eprintln!("puricat: {e}");
            std::process::exit(1);
        }
        return;
    }
    // Started by macOS for the scheduled backups: run the jobs that are due and end. No window.
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() == Some(std::ffi::OsStr::new("--run-backups")) {
        let data_dir = match (args.next(), args.next()) {
            (Some(flag), Some(dir)) if flag == "--data-dir" => std::path::PathBuf::from(dir),
            _ => {
                eprintln!("puricat: --run-backups needs --data-dir <directory>");
                std::process::exit(2);
            }
        };
        if let Err(e) = puricat_lib::run_backups(&data_dir) {
            eprintln!("puricat: the backup runner could not work: {e}");
            std::process::exit(1);
        }
        return;
    }
    puricat_lib::run()
}
