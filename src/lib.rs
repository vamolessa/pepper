pub mod application;
pub mod buffer;
pub mod buffer_position;
pub mod buffer_view;
pub mod client;
pub mod command;
pub mod config;
pub mod cursor;
pub mod editor;
pub mod editor_utils;
pub mod events;
pub mod glob;
pub mod help;
pub mod history;
pub mod ini;
pub mod json;
pub mod keymap;
pub mod lsp;
pub mod mode;
pub mod navigation_history;
pub mod pattern;
pub mod picker;
pub mod platform;
pub mod register;
pub mod serialization;
pub mod syntax;
pub mod theme;
pub mod ui;
pub mod word_database;

pub struct ArgsConfig {
    pub path: String,
    pub suppress_file_not_found: bool,
}

#[derive(Default)]
pub struct Args {
    pub version: bool,
    pub session: Option<String>,
    pub print_session: bool,
    pub as_focused_client: bool,
    pub quit: bool,
    pub server: bool,
    pub configs: Vec<ArgsConfig>,
    pub no_default_config: bool,
    pub files: Vec<String>,
}

fn print_version() {
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");
    println!("{} version {}", name, version);
}

fn print_help() {
    print_version();
    println!("{}", env!("CARGO_PKG_DESCRIPTION"));
    println!();
    println!("usage: pepper [<options...>] [<files...>]");
    println!();
    println!("  files: file paths to open as a buffer (clients only)");
    println!("         you can append ':<line>[,<column>]' to open it at that position");
    println!();
    println!("options:");
    println!();
    println!("  -h, --help               prints help and quits");
    println!("  -v, --version            prints version and quits");
    println!("  -s, --session            overrides the session name to connect to");
    println!("  --print-session          prints the computed session name and quits");
    println!("  --as-focused-client      sends events as if it was the currently focused client");
    println!("  --quit                   sends a `quit` event on start");
    println!("  --server                 only run as server");
    println!("  -c, --config             sources config file at path (repeatable) (server only)");
    println!("  --try-config             like `--config` but suppresses the 'file not found' error (repeatable)");
    println!(
        "  --no-default-config      does not source the default config included in the editor"
    );
}

impl Args {
    pub fn parse() -> Self {
        fn error(message: std::fmt::Arguments) -> ! {
            println!("{}", message);
            std::process::exit(0);
        }

        fn arg_to_str(arg: &std::ffi::OsString) -> &str {
            match arg.to_str() {
                Some(arg) => arg,
                None => error(format_args!("could not parse arg {:?}", arg)),
            }
        }

        let mut args = std::env::args_os();
        args.next();

        let mut parsed = Args::default();
        while let Some(arg) = args.next() {
            let arg = arg_to_str(&arg);
            match arg {
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                "-v" | "--version" => {
                    print_version();
                    std::process::exit(0);
                }
                "-s" | "--session" => match args.next() {
                    Some(arg) => {
                        let arg = arg_to_str(&arg);
                        if !arg.chars().all(char::is_alphanumeric) {
                            error(format_args!(
                                "invalid session name '{}'. it can only contain alphanumeric characters", arg
                            ));
                        }
                        parsed.session = Some(arg.into());
                    }
                    None => error(format_args!("expected session after {}", arg)),
                },
                "--print-session" => parsed.print_session = true,
                "--as-focused-client" => parsed.as_focused_client = true,
                "--quit" => parsed.quit = true,
                "--server" => parsed.server = true,
                "-c" | "--config" => match args.next() {
                    Some(arg) => {
                        let arg = arg_to_str(&arg);
                        parsed.configs.push(ArgsConfig {
                            path: arg.into(),
                            suppress_file_not_found: false,
                        });
                    }
                    None => error(format_args!("expected config path after {}", arg)),
                },
                "--try-config" => match args.next() {
                    Some(arg) => {
                        let arg = arg_to_str(&arg);
                        parsed.configs.push(ArgsConfig {
                            path: arg.into(),
                            suppress_file_not_found: true,
                        });
                    }
                    None => error(format_args!("expected config path after {}", arg)),
                },
                "--no-default-config" => parsed.no_default_config = true,
                "--" => {
                    while let Some(arg) = args.next() {
                        let arg = arg_to_str(&arg);
                        parsed.files.push(arg.into());
                    }
                }
                _ => {
                    if arg.starts_with('-') {
                        error(format_args!("invalid option '{}'", arg));
                    } else {
                        parsed.files.push(arg.into());
                    }
                }
            }
        }

        parsed
    }
}

