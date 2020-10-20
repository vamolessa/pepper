#![macro_use]

macro_rules! unwrap_or_return {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return,
        }
    };
}

macro_rules! unwrap_or_none {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => return ModeOperation::None,
        }
    };
}

macro_rules! impl_from_script {
    ($type:ty, $from_value:ident => $from:expr) => {
        impl<'lua> mlua::FromLua<'lua> for $type {
            fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
                let $from_value = ScriptValue::from_lua(lua_value, lua)?;
                match $from {
                    Some(value) => Ok(value),
                    None => Err(mlua::Error::FromLuaConversionError {
                        from: $from_value.type_name(),
                        to: std::any::type_name::<$type>(),
                        message: None,
                    }),
                }
            }
        }
    };
}

macro_rules! impl_to_script {
    ($type:ty, ($to_value:ident, $engine:ident) => $to:expr) => {
        impl<'lua> mlua::ToLua<'lua> for $type {
            fn to_lua($to_value: Self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value> {
                let $engine = $crate::script::ScriptEngineRef::from_lua(lua);
                $to.to_lua(lua)
            }
        }
    };
}

