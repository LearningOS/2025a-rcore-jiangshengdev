//! 全局日志记录器

use log::{Level, LevelFilter, Log, Metadata, Record};

/// 一个简单的日志记录器
struct SimpleLogger;

impl Log for SimpleLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // 始终启用日志记录
        true
    }
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // 根据日志级别选择不同的颜色
        let color = match record.level() {
            Level::Error => 31, // 红色
            Level::Warn => 93,  // 亮黄色
            Level::Info => 34,  // 蓝色
            Level::Debug => 32, // 绿色
            Level::Trace => 90, // 亮黑色
        };
        // 使用ANSI转义序列输出带颜色的日志
        println!(
            "\u{1B}[{}m[{:>5}] {}\u{1B}[0m",
            color,
            record.level(),
            record.args(),
        );
    }
    fn flush(&self) {}
}

/// 初始化日志记录器
pub fn init() {
    static LOGGER: SimpleLogger = SimpleLogger;
    // 设置全局日志记录器
    log::set_logger(&LOGGER).unwrap();
    // 根据环境变量LOG设置日志级别
    log::set_max_level(match option_env!("LOG") {
        Some("ERROR") => LevelFilter::Error,
        Some("WARN") => LevelFilter::Warn,
        Some("INFO") => LevelFilter::Info,
        Some("DEBUG") => LevelFilter::Debug,
        Some("TRACE") => LevelFilter::Trace,
        _ => LevelFilter::Off,
    });
}
