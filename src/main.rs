fn main() {
    env_logger::init();

    if let Err(error) = gbrust::gui::run() {
        log::error!("Application error: {}", error);
        std::process::exit(1);
    }
}
