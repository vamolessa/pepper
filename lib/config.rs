use std::{fmt, num::NonZeroU8};

pub enum ConfigError {
    NotFound,
    InvalidValue,
}

macro_rules! config_values {
    ($($name:ident: $type:ty = $default:expr,)*) => {
        pub struct Config {
            $(pub $name: $type,)*
        }

        impl Config {
            pub fn parse_config(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
                match key {
                    $(stringify!($name) => match value.parse() {
                        Ok(value) => self.$name = value,
                        Err(_) => return Err(ConfigError::InvalidValue),
                    },)*
                    _ => return Err(ConfigError::NotFound),
                }
                Ok(())
            }

            pub fn display_config(&mut self, key: &str, formatter: &mut fmt::Formatter) -> Result<(), ConfigError> {
                match key {
                    $(stringify!($name) => match formatter.write_fmt(format_args!("{}", &self.$name)) {
                        Ok(()) => (),
                        Err(_) => return Err(ConfigError::InvalidValue),
                    },)*
                    _ => return Err(ConfigError::NotFound),
                }
                Ok(())
            }
        }

        impl Default for Config {
            fn default() -> Self {
                Self {
                    $($name: $default,)*
                }
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

    picker_max_height: NonZeroU8 = NonZeroU8::new(8).unwrap(),
}
