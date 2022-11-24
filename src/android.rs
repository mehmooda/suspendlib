use android_logger::{init_once, Config};
use log::Level;

pub fn init() {
    init_once(
        Config::default().with_min_level(Level::Debug)
    );
    log_panics::Config::new().backtrace_mode(log_panics::BacktraceMode::Resolved).install_panic_hook();
}