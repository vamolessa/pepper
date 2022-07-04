use std::{fmt, num::NonZeroU8};

pub enum ParseConfigError {
    NoSuchConfig,
    InvalidValue,
}
impl fmt::Display for ParseConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::NoSuchConfig => f.write_str("no such config"),
            Self::InvalidValue => f.write_str("invalid config value"),
        }
    }
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
    indent_with_tabs: bool = false,

    visual_empty: char = '~',
    visual_space: char = '.',
    visual_tab_first: char = '|',
    visual_tab_repeat: char = ' ',

    completion_min_len: u8 = 3,
    picker_max_height: u8 = 8,
    status_bar_max_height: NonZeroU8 = NonZeroU8::new(8).unwrap(),
}
