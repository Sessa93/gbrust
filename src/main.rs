fn main() {
    env_logger::init();

    if let Err(e) = gbrust::gui::run() {
        log::error!("Application error: {}", e);
        std::process::exit(1);
    }
}
