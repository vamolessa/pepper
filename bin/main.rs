#[cfg(windows)]
mod windows;

fn main() {
    #[cfg(windows)]
    windows::main();
}
