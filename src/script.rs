use std::{error::Error, fmt, fs::File, io::Read, path::Path, sync::Arc};

use mlua::prelude::{
    FromLua, FromLuaMulti, Lua, LuaError, LuaFunction, LuaInteger, LuaLightUserData, LuaNumber,
    LuaResult, LuaString, LuaValue, ToLuaMulti,
};

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    config::Config,
    editor::{EditorLoop, StatusMessageKind},
    keymap::KeyMapCollection,
    script_bindings,
    picker::Picker,
};

pub type ScriptResult<T> = LuaResult<T>;

pub struct ScriptError<T>(T);
impl<T> ScriptError<T>
where
    T: 'static + fmt::Display,
{
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

pub struct ScriptStr<'lua>(LuaString<'lua>);
impl<'lua> ScriptStr<'lua> {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
    pub fn to_str(&self) -> ScriptResult<&str> {
        self.0.to_str()
    }
}
impl<'lua> FromLua<'lua> for ScriptStr<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _lua: &'lua Lua) -> LuaResult<Self> {
        if let LuaValue::String(s) = lua_value {
            Ok(Self(s))
        } else {
            Err(LuaError::FromLuaConversionError {
                from: lua_value.type_name(),
                to: stringify!(ScriptStr),
                message: None,
            })
        }
    }
}

pub struct ScriptFunction<'lua>(LuaFunction<'lua>);
impl<'lua> ScriptFunction<'lua> {
    pub fn call<A, R>(&self, args: A) -> ScriptResult<R>
    where
        A: ToLuaMulti<'lua>,
        R: FromLuaMulti<'lua>,
    {
        self.0.call(args)
    }
}
impl<'lua> FromLua<'lua> for ScriptFunction<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _lua: &'lua Lua) -> LuaResult<Self> {
        if let LuaValue::Function(f) = lua_value {
            Ok(Self(f))
        } else {
            Err(LuaError::FromLuaConversionError {
                from: lua_value.type_name(),
                to: stringify!(ScriptFunction),
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
    String(ScriptStr<'lua>),
    Function(ScriptFunction<'lua>),
    Other(LuaValue<'lua>),
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
            ScriptValue::Function(_) => f.write_str("function"),
            ScriptValue::Other(_) => Ok(()),
        }
    }
}
impl<'lua> FromLua<'lua> for ScriptValue<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _lua: &'lua Lua) -> LuaResult<Self> {
        match lua_value {
            LuaValue::Nil => Ok(Self::Nil),
            LuaValue::Boolean(b) => Ok(Self::Boolean(b)),
            LuaValue::Integer(i) => Ok(Self::Integer(i)),
            LuaValue::Number(n) => Ok(Self::Number(n)),
            LuaValue::String(s) => Ok(Self::String(ScriptStr(s))),
            LuaValue::Function(f) => Ok(Self::Function(ScriptFunction(f))),
            _ => Ok(Self::Other(lua_value)),
        }
    }
}

pub struct ScriptContext<'a> {
    pub target_client: TargetClient,
    pub clients: &'a mut ClientCollection,
    pub editor_loop: &'a mut EditorLoop,

    pub config: &'a mut Config,

    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,

    pub picker: &'a mut Picker,

    pub status_message_kind: &'a mut StatusMessageKind,
    pub status_message: &'a mut String,

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

        let mut this = Self { lua };
        script_bindings::bind_all(&mut this)?;

        Ok(this)
    }

    pub fn register_ctx_function<'lua, A, R, F>(
        &'lua mut self,
        name: &str,
        func: F,
    ) -> ScriptResult<()>
    where
        A: FromLuaMulti<'lua>,
        R: ToLuaMulti<'lua>,
        F: 'static + Fn(&mut ScriptContext, A) -> ScriptResult<R>,
    {
        let func = self.lua.create_function(move |lua, args| {
            let ctx: LuaLightUserData = lua.named_registry_value("ctx")?;
            let ctx = unsafe { &mut *(ctx.0 as *mut _) };
            func(ctx, args)
        })?;
        self.lua.globals().set(name, func)?;
        Ok(())
    }

    pub fn eval(&mut self, mut ctx: ScriptContext, source: &str) -> ScriptResult<ScriptValue> {
        self.update_ctx(&mut ctx)?;
        self.lua.load(source).set_name(source)?.eval()
    }

    pub fn eval_entry_file(&mut self, mut ctx: ScriptContext, path: &Path) -> ScriptResult<()> {
        let mut file = File::open(path).map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let metadata = file
            .metadata()
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let mut source = String::with_capacity(metadata.len() as _);
        file.read_to_string(&mut source)
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;

        self.update_ctx(&mut ctx)?;

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
