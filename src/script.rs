#![macro_use]

use std::{convert::TryInto, error::Error, fmt, fs::File, io::Read, path::Path, sync::Arc};

use mlua::prelude::{
    FromLua, FromLuaMulti, Lua, LuaError, LuaFunction, LuaInteger, LuaLightUserData, LuaNumber,
    LuaResult, LuaString, LuaTable, LuaValue, ToLua, ToLuaMulti,
};

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    config::Config,
    editor::{EditorLoop, StatusMessage},
    keymap::KeyMapCollection,
    mode::Mode,
    picker::Picker,
    script_bindings,
    word_database::WordDatabase,
};

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
    ($type:ty, $to_value:ident => $to:expr) => {
        impl<'lua> mlua::ToLua<'lua> for $type {
            fn to_lua($to_value: Self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value> {
                $to.to_lua(lua)
            }
        }
    };
}

pub type ScriptResult<T> = LuaResult<T>;

pub struct ScriptError<T>(T);
impl<T> ScriptError<T>
where
    T: 'static + fmt::Display,
{
    pub fn convert_from_script(from: &ScriptValue) -> LuaError {
        LuaError::FromLuaConversionError {
            from: from.type_name(),
            to: std::any::type_name::<T>(),
            message: None,
        }
    }

    pub fn from(e: T) -> LuaError {
        LuaError::ExternalError(Arc::new(ScriptError(e)))
    }
}
impl<T> fmt::Debug for ScriptError<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl<T> fmt::Display for ScriptError<T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl<T> Error for ScriptError<T> where T: fmt::Display {}

pub struct ScriptString<'lua>(LuaString<'lua>);
impl<'lua> ScriptString<'lua> {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
    pub fn to_str(&self) -> ScriptResult<&str> {
        self.0.to_str()
    }
}
impl<'lua> FromLua<'lua> for ScriptString<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        if let LuaValue::String(s) = lua_value {
            Ok(Self(s))
        } else {
            Err(LuaError::FromLuaConversionError {
                from: lua_value.type_name(),
                to: std::any::type_name::<Self>(),
                message: None,
            })
        }
    }
}

pub struct ScriptObject<'lua>(LuaTable<'lua>);
impl<'lua> ScriptObject<'lua> {
    pub fn get<T>(&self, key: &str) -> ScriptResult<T>
    where
        T: FromLua<'lua>,
    {
        self.0.get(key)
    }

    pub fn set<T>(&self, key: &str, value: T) -> ScriptResult<()>
    where
        T: ToLua<'lua>,
    {
        self.0.set(key, value)
    }

    pub fn set_meta_object(&self, object: Option<ScriptObject<'lua>>) {
        self.0.set_metatable(object.map(|o| o.0))
    }
}
impl<'lua> FromLua<'lua> for ScriptObject<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        if let LuaValue::Table(t) = lua_value {
            Ok(Self(t))
        } else {
            Err(LuaError::FromLuaConversionError {
                from: lua_value.type_name(),
                to: std::any::type_name::<Self>(),
                message: None,
            })
        }
    }
}

pub struct ScriptFunction<'lua>(LuaFunction<'lua>);
impl<'lua> ScriptFunction<'lua> {
    // it mutably borrows ScriptContext to make sure it does not alias
    pub fn call<A, R>(&self, _: &mut ScriptContext, args: A) -> ScriptResult<R>
    where
        A: ToLuaMulti<'lua>,
        R: FromLuaMulti<'lua>,
    {
        self.0.call(args)
    }
}
impl<'lua> FromLua<'lua> for ScriptFunction<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        if let LuaValue::Function(f) = lua_value {
            Ok(Self(f))
        } else {
            Err(LuaError::FromLuaConversionError {
                from: lua_value.type_name(),
                to: std::any::type_name::<Self>(),
                message: None,
            })
        }
    }
}

pub enum ScriptValue<'lua> {
    Nil,
    Boolean(bool),
    Integer(LuaInteger),
    Number(LuaNumber),
    String(ScriptString<'lua>),
    Object(ScriptObject<'lua>),
    Function(ScriptFunction<'lua>),
}
impl<'lua> ScriptValue<'lua> {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Boolean(_) => "boolean",
            Self::Integer(_) => "integer",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Object(_) => "table",
            Self::Function(_) => "function",
        }
    }
}
impl<'lua> fmt::Display for ScriptValue<'lua> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ScriptValue::Nil => f.write_str("nil"),
            ScriptValue::Boolean(b) => b.fmt(f),
            ScriptValue::Integer(i) => i.fmt(f),
            ScriptValue::Number(n) => n.fmt(f),
            ScriptValue::String(s) => match s.to_str() {
                Ok(s) => s.fmt(f),
                Err(_) => Err(fmt::Error),
            },
            ScriptValue::Object(_) => f.write_str("object"),
            ScriptValue::Function(_) => f.write_str("function"),
        }
    }
}
impl<'lua> FromLua<'lua> for ScriptValue<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        match lua_value {
            LuaValue::Nil => Ok(Self::Nil),
            LuaValue::Boolean(b) => Ok(Self::Boolean(b)),
            LuaValue::Integer(i) => Ok(Self::Integer(i)),
            LuaValue::Number(n) => Ok(Self::Number(n)),
            LuaValue::String(s) => Ok(Self::String(ScriptString(s))),
            LuaValue::Table(t) => Ok(Self::Object(ScriptObject(t))),
            LuaValue::Function(f) => Ok(Self::Function(ScriptFunction(f))),
            _ => Err(LuaError::FromLuaConversionError {
                from: lua_value.type_name(),
                to: std::any::type_name::<Self>(),
                message: None,
            }),
        }
    }
}
impl<'lua> ToLua<'lua> for ScriptValue<'lua> {
    fn to_lua(self, _: &'lua Lua) -> LuaResult<LuaValue<'lua>> {
        match self {
            Self::Nil => Ok(LuaValue::Nil),
            Self::Boolean(b) => Ok(LuaValue::Boolean(b)),
            Self::Integer(i) => Ok(LuaValue::Integer(i)),
            Self::Number(n) => Ok(LuaValue::Number(n)),
            Self::String(s) => Ok(LuaValue::String(s.0)),
            Self::Object(o) => Ok(LuaValue::Table(o.0)),
            Self::Function(f) => Ok(LuaValue::Function(f.0)),
        }
    }
}
impl<'lua> TryInto<char> for ScriptValue<'lua> {
    type Error = ();

    fn try_into(self) -> Result<char, Self::Error> {
        match self {
            Self::String(s) => {
                let mut chars = s.to_str().map_err(|_| ())?.chars();
                let c = chars.next();
                match (c, chars.next()) {
                    (Some(c), None) => Ok(c),
                    _ => Err(()),
                }
            }
            _ => Err(()),
        }
    }
}

pub struct ScriptContext<'a> {
    pub target_client: TargetClient,
    pub clients: &'a mut ClientCollection,
    pub editor_loop: EditorLoop,
    pub next_mode: Mode,

    pub config: &'a mut Config,

    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub word_database: &'a mut WordDatabase,

    pub picker: &'a mut Picker,

    pub status_message: &'a mut StatusMessage,

    pub keymaps: &'a mut KeyMapCollection,
}

impl<'a> ScriptContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.clients
            .get(self.target_client)
            .and_then(|c| c.current_buffer_view_handle)
    }

    pub fn set_current_buffer_view_handle(&mut self, handle: Option<BufferViewHandle>) {
        if let Some(client) = self.clients.get_mut(self.target_client) {
            client.current_buffer_view_handle = handle;
        }
    }
}

pub struct ScriptEngine {
    lua: Lua,
}

impl ScriptEngine {
    pub fn new() -> Self {
        Self::try_new().unwrap()
    }

    pub fn try_new() -> ScriptResult<Self> {
        let libs = mlua::StdLib::TABLE
            | mlua::StdLib::STRING
            | mlua::StdLib::UTF8
            | mlua::StdLib::MATH
            | mlua::StdLib::PACKAGE;
        let lua = Lua::new_with(libs)?;

        let this = Self { lua };
        script_bindings::bind_all(this.as_ref())?;

        Ok(this)
    }

    pub fn as_ref(&self) -> ScriptEngineRef {
        ScriptEngineRef { lua: &self.lua }
    }

    pub fn eval(&mut self, ctx: &mut ScriptContext, source: &str) -> ScriptResult<ScriptValue> {
        self.update_ctx(ctx)?;
        self.lua.load(source).set_name(source)?.eval()
    }

    pub fn eval_entry_file(&mut self, ctx: &mut ScriptContext, path: &Path) -> ScriptResult<()> {
        let mut file = File::open(path).map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let metadata = file
            .metadata()
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let mut source = String::with_capacity(metadata.len() as _);
        file.read_to_string(&mut source)
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;

        self.update_ctx(ctx)?;

        let chunk = self.lua.load(&source);
        if let Some(name) = path.to_str() {
            chunk.set_name(name)?.exec()?;
        } else {
            chunk.exec()?;
        }

        Ok(())
    }

    fn update_ctx(&mut self, ctx: &mut ScriptContext) -> ScriptResult<()> {
        self.lua
            .set_named_registry_value("ctx", LuaLightUserData(ctx as *mut ScriptContext as _))
    }
}

#[derive(Clone, Copy)]
pub struct ScriptEngineRef<'lua> {
    lua: &'lua Lua,
}

impl<'lua> ScriptEngineRef<'lua> {
    pub fn globals_object(&self) -> ScriptObject<'lua> {
        ScriptObject(self.lua.globals())
    }

    pub fn create_string(&self, data: &[u8]) -> ScriptResult<ScriptString<'lua>> {
        self.lua.create_string(data).map(ScriptString)
    }

    pub fn create_object(&self) -> ScriptResult<ScriptObject<'lua>> {
        self.lua.create_table().map(ScriptObject)
    }

    pub fn create_ctx_function<A, R, F>(&self, func: F) -> ScriptResult<ScriptFunction<'lua>>
    where
        A: FromLuaMulti<'lua>,
        R: ToLuaMulti<'lua>,
        F: 'static + Fn(ScriptEngineRef<'lua>, &mut ScriptContext, A) -> ScriptResult<R>,
    {
        self.lua
            .create_function(move |lua, args| {
                let ctx: LuaLightUserData = lua.named_registry_value("ctx")?;
                let ctx = unsafe { &mut *(ctx.0 as *mut _) };
                let engine = ScriptEngineRef { lua };
                func(engine, ctx, args)
            })
            .map(ScriptFunction)
    }

    pub fn save_to_registry<T>(&self, key: &str, value: T) -> ScriptResult<()>
    where
        T: ToLua<'lua>,
    {
        self.lua.set_named_registry_value(key, value)
    }

    pub fn take_from_registry<T>(&self, key: &str) -> ScriptResult<T>
    where
        T: FromLua<'lua>,
    {
        let value = self.lua.named_registry_value(key)?;
        self.lua.unset_named_registry_value(key)?;
        Ok(value)
    }
}
