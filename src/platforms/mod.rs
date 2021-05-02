#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
))
mod bsd;

pub fn main() {
    #[cfg(windows)]
    windows::main();

    #[cfg(target_os = "linux")]
    linux::main();
    
    #[cfg(target_os = "bsd")]
    bsd::main();
}
