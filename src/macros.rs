#![macro_use]

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

macro_rules! impl_script_userdata {
    ($type:ty) => {
        impl mlua::UserData for $type {}
    };
}

macro_rules! declare_json_object {
    (
        $(#[$attribute:meta])*
        $struct_vis:vis
        struct $struct_name:ident {
            $($member_vis:vis $member_name:ident : $member_type:ty,)*
        }
    ) => {
        #[allow(non_snake_case)]
        $(#[$attribute])*
        $struct_vis struct $struct_name {
            $($member_vis $member_name : $member_type,)*
        }
        impl<'json> $crate::json::FromJson<'json> for $struct_name {
            fn from_json(
                value: $crate::json::JsonValue,
                json: &'json $crate::json::Json,
            ) -> Result<Self, $crate::json::JsonConvertError> {
                match value {
                    JsonValue::Object(object) => {
                        let mut this = Self {
                            $($member_name : Default::default(),)*
                        };
                        for (key, value) in object.members(json) {
                            match key {
                                $(stringify!($member_name) => {
                                    this.$member_name = $crate::json::FromJson::from_json(value, json)?
                                })*
                                _ => (),
                            }
                        }
                        Ok(this)
                    }
                    _ => Err($crate::json::JsonConvertError)
                }
            }
        }
    }
}
