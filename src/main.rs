use std::{fs, io, mem::MaybeUninit, panic};

#[cfg(windows)]
#[path = "platforms/windows.rs"]
mod sys;

#[cfg(target_os = "linux")]
#[path = "platforms/linux.rs"]
mod sys;

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]
#[path = "platforms/bsd.rs"]
mod sys;

fn main() {
    static mut ORIGINAL_PANIC_HOOK: MaybeUninit<Box<dyn Fn(&panic::PanicInfo) + Sync + Send>> =
        MaybeUninit::uninit();
    unsafe { ORIGINAL_PANIC_HOOK = MaybeUninit::new(panic::take_hook()) };

    panic::set_hook(Box::new(|info| unsafe {
        if let Ok(mut file) = fs::File::create("pepper-crash.txt") {
            use io::Write;
            let _ = writeln!(file, "{}", info);
        }

        sys::try_launching_debugger();

        let hook = ORIGINAL_PANIC_HOOK.assume_init_ref();
        hook(info);
    }));

    sys::main();
}

