#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use soziopolis_lingq_tool::{gui, logging};

fn main() -> iced::Result {
    logging::install_panic_hook();
    if let Ok(log_path) = logging::init() {
        logging::info(format!(
            "application start; log path {}",
            log_path.display()
        ));
    }
    logging::info("launching GUI");
    gui::run()
}
