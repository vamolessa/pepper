use pepper::application::set_panic_hook;

mod platforms;

fn main() {
    set_panic_hook();
    platforms::main();
}
