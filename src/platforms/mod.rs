#[cfg(windows)]
mod windows;

pub fn main() {
    #[cfg(windows)]
    windows::main();
}
