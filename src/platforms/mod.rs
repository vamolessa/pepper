#[cfg(windows)]
mod platform {
    #[path = "../windows.rs"]
    pub mod sys;
}

#[cfg(target_os = "linux")]
mod platform {
    #[path = "../linux.rs"]
    pub mod sys;
}

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))]
mod platform {
    #[path = "../bsd.rs"]
    pub mod sys;
}

pub fn main() {
    platform::sys::main();
}

