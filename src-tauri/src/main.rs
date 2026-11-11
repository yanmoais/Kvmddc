#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(err) = kvmddc_app_lib::run_entry() {
        eprintln!("Kvmddc error: {err:#}");
        std::process::exit(1);
    }
}
