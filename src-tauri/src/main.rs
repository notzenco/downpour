// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if downpour_lib::browser::is_host_invocation(&args) {
        std::process::exit(downpour_lib::browser::run_host());
    }
    downpour_lib::run()
}
