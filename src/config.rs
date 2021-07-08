use std::{fmt, num::NonZeroU8};

use crate::{
    editor::Editor,
    editor_utils::{EditorOutputWrite, MessageKind},
    ini::{Ini, PropertyIterator},
    keymap::{KeyMapCollection, ParseKeyMapError},
    mode::ModeKind,
    syntax::{Syntax, TokenKind},
    theme::Color,
};

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
    fn parse_bindings(
        keymaps: &mut KeyMapCollection,
        mode: ModeKind,
        bindings: PropertyIterator,
        config_name: &str,
        output: &mut EditorOutputWrite,
    ) {
        for (key, value, line_index) in bindings {
            match keymaps.parse_and_map(mode, key, value) {
                Ok(()) => (),
                Err(ParseKeyMapError::From(error)) => output.fmt(format_args!(
                    "invalid from binding '{}' at {}:{}",
                    key, config_name, line_index,
                )),
                Err(ParseKeyMapError::To(error)) => output.fmt(format_args!(
                    "invalid to binding '{}' at {}:{}",
                    value, config_name, line_index,
                ))
            }
        }
    }

    let mut output = editor.status_bar.write(MessageKind::Error);

    if let Err(error) = ini.parse(config_content) {
        output.fmt(format_args!(
            "error parsing config {}:{} : {}",
            config_name, error.line_index, error.kind,
        ));
        return;
    }

    'section_loop: for (section, line_index, properties) in ini.sections() {
        match section {
            "config" => {
                for (key, value, line_index) in properties {
                    match editor.config.parse_config(key, value) {
                        Ok(()) => (),
                        Err(ParseConfigError::NoSuchConfig) => output.fmt(format_args!(
                            "no such config '{}' at {}:{}\n",
                            key, config_name, line_index
                        )),
                        Err(ParseConfigError::InvalidValue) => output.fmt(format_args!(
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
                            output.fmt(format_args!(
                                "no such color '{}' at {}:{}\n",
                                key, config_name, line_index
                            ));
                            continue;
                        }
                    };
                    let encoded = match u32::from_str_radix(value, 16) {
                        Ok(value) => value,
                        Err(_) => {
                            output.fmt(format_args!(
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
                let mut syntax = Syntax::new();
                let mut has_glob = false;
                for (key, value, line_index) in properties {
                    match key {
                        "glob" => match syntax.set_glob(value) {
                            Ok(()) => has_glob = true,
                            Err(_) => {
                                output.fmt(format_args!(
                                    "invalid glob '{}' at {}:{}",
                                    value, config_name, line_index,
                                ));
                                continue 'section_loop;
                            }
                        },
                        _ => match key.parse() {
                            Ok(kind) => match syntax.set_rule(kind, value) {
                                Ok(()) => (),
                                Err(error) => {
                                    output.fmt(format_args!(
                                        "syntax pattern error '{}' at {}:{}",
                                        error, config_name, line_index,
                                    ));
                                    continue 'section_loop;
                                }
                            },
                            Err(_) => {
                                output.fmt(format_args!(
                                    "no such token kind '{}' at {}:{}",
                                    key, config_name, line_index
                                ));
                                continue 'section_loop;
                            }
                        },
                    }
                }

                if !has_glob {
                    output.fmt(format_args!(
                        "syntax has no glob property at {}:{}",
                        config_name, line_index,
                    ));
                    continue;
                }

                editor.syntaxes.add(syntax);
            }
            "normal-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Normal,
                properties,
                config_name,
                &mut output,
            ),
            "insert-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Insert,
                properties,
                config_name,
                &mut output,
            ),
            "command-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Command,
                properties,
                config_name,
                &mut output,
            ),
            "readline-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::ReadLine,
                properties,
                config_name,
                &mut output,
            ),
            "picker-bindings" => parse_bindings(
                &mut editor.keymaps,
                ModeKind::Picker,
                properties,
                config_name,
                &mut output,
            ),
            _ => output.fmt(format_args!(
                "no such config '{}' at {}:{}\n",
                section, config_name, line_index,
            )),
        }
    }
}

