#![macro_use]

macro_rules! declare_json_object {
    (
        $(#[$attribute:meta])*
        struct $struct_name:ident {
            $($member_name:ident : $member_type:ty,)*
        }
    ) => {
        #[allow(non_snake_case)]
        $(#[$attribute])*
        struct $struct_name {
            pub $($member_name : $member_type,)*
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
