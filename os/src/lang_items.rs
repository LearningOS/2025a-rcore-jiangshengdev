//! panic 处理程序

use crate::sbi::shutdown;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // 检查是否有位置信息
    if let Some(location) = info.location() {
        // 打印详细的panic信息，包括文件名、行号和消息
        println!(
            "[kernel] Panicked at {}:{} {}",
            location.file(),
            location.line(),
            info.message().unwrap()
        );
    } else {
        // 只打印panic消息
        println!("[kernel] Panicked: {}", info.message().unwrap());
    }
    // 关闭系统
    shutdown()
}
