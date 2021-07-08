use std::{fmt, num::NonZeroU8};

use crate::{editor::Editor, editor_utils::MessageKind, ini::Ini, theme::Color};

pub enum ParseConfigError {
    NoSuchConfig,
    InvalidValue,
}

macro_rules! config_values {
    ($($name:ident: $type:ty = $default:expr,)*) => {
        pub static CONFIG_NAMES: &[&str] = &[$(stringify!($name),)*];

        pub struct Config {
            $(pub $name: $type,)*
        }

        impl Config {
            pub fn parse_config(&mut self, key: &str, value: &str) -> Result<(), ParseConfigError> {
                match key {
                    $(stringify!($name) => match value.parse() {
                        Ok(value) => self.$name = value,
                        Err(_) => return Err(ParseConfigError::InvalidValue),
                    },)*
                    _ => return Err(ParseConfigError::NoSuchConfig),
                }
                Ok(())
            }

            pub fn display_config(&self, key: &str) -> Option<DisplayConfig> {
                match key {
                    $(stringify!($name) => Some(DisplayConfig {
                        config: self,
                        writter: |c, f| fmt::Display::fmt(&c.$name, f),
                    }),)*
                    _ => None,
                }
            }
        }

        impl Default for Config {
            fn default() -> Self {
                Self {
                    $($name: $default,)*
                }
            }
        }

        pub struct DisplayConfig<'a> {
            config: &'a Config,
            writter: fn(&Config, &mut fmt::Formatter) -> fmt::Result
        }

        impl<'a> fmt::Display for DisplayConfig<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                (self.writter)(self.config, f)
            }
        }
    }
}

config_values! {
    tab_size: NonZeroU8 = NonZeroU8::new(4).unwrap(),
    indent_with_tabs: bool = true,

    visual_empty: u8 = b'~',
    visual_space: u8 = b'.',
    visual_tab_first: u8 = b'|',
    visual_tab_repeat: u8 = b' ',

    completion_min_len: u8 = 3,
    picker_max_height: u8 = 8,
}

pub fn load_config<'content>(
    editor: &mut Editor,
    ini: &mut Ini<'content>,
    config_name: &str,
    config_content: &'content str,
) {
    let mut write = editor.status_bar.write(MessageKind::Error);

    if let Err(error) = ini.parse(config_content) {
        write.fmt(format_args!(
            "error parsing config {}:{} : {}",
            config_name, error.line_index, error.kind,
        ));
        return;
    }

    for (section, line_index, properties) in ini.sections() {
        match section {
            "config" => {
                for (key, value, line_index) in properties {
                    match editor.config.parse_config(key, value) {
                        Ok(()) => (),
                        Err(ParseConfigError::NoSuchConfig) => write.fmt(format_args!(
                            "no such config '{}' at {}:{}\n",
                            key, config_name, line_index
                        )),
                        Err(ParseConfigError::InvalidValue) => write.fmt(format_args!(
                            "invalid config value '{}' at {}:{}\n",
                            value, config_name, line_index,
                        )),
                    }
                }
            }
            "theme" => {
                for (key, value, line_index) in properties {
                    let color = match editor.theme.color_from_name(key) {
                        Some(color) => color,
                        None => {
                            write.fmt(format_args!(
                                "no such color '{}' at {}:{}\n",
                                key, config_name, line_index
                            ));
                            continue;
                        }
                    };
                    let encoded = match u32::from_str_radix(value, 16) {
                        Ok(value) => value,
                        Err(_) => {
                            write.fmt(format_args!(
                                "invalid color value '{}' at {}:{}\n",
                                value, config_name, line_index
                            ));
                            continue;
                        }
                    };
                    *color = Color::from_u32(encoded);
                }
            }
            "syntax" => {
                todo!();
            }
            "normal-bindings" => {
                todo!();
            }
            "insert-bindings" => {
                todo!();
            }
            "command-bindings" => {
                todo!();
            }
            "readline-bindings" => {
                todo!();
            }
            "picker-bindings" => {
                todo!();
            }
            _ => write.fmt(format_args!(
                "no such config '{}' at {}:{}\n",
                section, config_name, line_index,
            )),
        }
    }
}

