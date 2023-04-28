use log::Log;
use owo_colors::OwoColorize;

struct Logger;

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn flush(&self) {}

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let message = format!(
            "[{}:{}] {}",
            record.file().unwrap(),
            record.line().unwrap(),
            record.args()
        );

        if record.level() == log::Level::Info {
            println!("{}", message.green());
        } else {
            println!("{}", message.bright_red());
        }
    }
}

pub fn init() {
    log::set_logger(&Logger).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}
