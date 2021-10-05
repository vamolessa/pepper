fn main() {
    let mut ctx = pepper::application::ApplicationContext::default();
    ctx.on_panic_config.write_info_to_file = true;
    ctx.on_panic_config.try_attaching_debugger = true;
    pepper::run(ctx);
}
