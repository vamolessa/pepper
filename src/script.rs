use std::{
    collections::VecDeque,
    convert::TryInto,
    error::Error,
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use mlua::prelude::{
    FromLua, FromLuaMulti, Lua, LuaError, LuaFunction, LuaInteger, LuaLightUserData, LuaNumber,
    LuaResult, LuaString, LuaTable, LuaTableSequence, LuaValue, ToLua, ToLuaMulti,
};

use crate::{
    buffer::{BufferCollection, BufferHandle},
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    config::Config,
    editor::{EditorEvent, EditorEventQueue, EditorLoop, StatusMessage},
    keymap::KeyMapCollection,
    mode::Mode,
    picker::Picker,
    register::RegisterCollection,
    script_bindings,
    word_database::WordDatabase,
};

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

pub struct ScriptArray<'lua>(LuaTable<'lua>);
impl<'lua> ScriptArray<'lua> {
    pub fn push<T>(&self, value: T) -> ScriptResult<()>
    where
        T: ToLua<'lua>,
    {
        self.0.set(self.0.len()? + 1, value)
    }

    pub fn iter<T>(self) -> LuaTableSequence<'lua, T>
    where
        T: FromLua<'lua>,
    {
        self.0.sequence_values()
    }
}
impl<'lua> FromLua<'lua> for ScriptArray<'lua> {
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
    pub fn call<A, R>(&self, _: &mut ScriptContextGuard, args: A) -> ScriptResult<R>
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
    Array(ScriptArray<'lua>),
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
            Self::Object(_) | Self::Array(_) => "table",
            Self::Function(_) => "function",
        }
    }
}
impl<'lua> fmt::Display for ScriptValue<'lua> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn fmt_recursive(value: &ScriptValue, f: &mut fmt::Formatter, depth: usize) -> fmt::Result {
            match value {
                ScriptValue::Nil => f.write_str("nil"),
                ScriptValue::Boolean(b) => b.fmt(f),
                ScriptValue::Integer(i) => i.fmt(f),
                ScriptValue::Number(n) => n.fmt(f),
                ScriptValue::String(s) => match s.to_str() {
                    Ok(s) => s.fmt(f),
                    Err(_) => Err(fmt::Error),
                },
                ScriptValue::Object(o) => {
                    f.write_str("{")?;
                    if depth == 0 {
                        f.write_str("...")?;
                    } else {
                        let o = o.0.clone();
                        for pair in o.pairs::<ScriptValue, ScriptValue>() {
                            if let Ok((key, value)) = pair {
                                fmt_recursive(&key, f, depth - 1)?;
                                f.write_str(":")?;
                                fmt_recursive(&value, f, depth - 1)?;
                                f.write_str(",")?;
                            }
                        }
                    }
                    f.write_str("}")?;
                    Ok(())
                }
                ScriptValue::Array(a) => {
                    f.write_str("[")?;
                    if depth == 0 {
                        f.write_str("...")?;
                    } else {
                        let a = a.0.clone();
                        for value in a.sequence_values::<ScriptValue>() {
                            if let Ok(value) = value {
                                fmt_recursive(&value, f, depth - 1)?;
                            }
                        }
                    }
                    f.write_str("]")?;
                    Ok(())
                }
                ScriptValue::Function(_) => f.write_str("function"),
            }
        }

        fmt_recursive(self, f, 2)
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
            Self::Array(a) => Ok(LuaValue::Table(a.0)),
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

    pub registers: &'a mut RegisterCollection,
    pub picker: &'a mut Picker,

    pub status_message: &'a mut StatusMessage,

    pub events: &'a mut EditorEventQueue,
    pub keymaps: &'a mut KeyMapCollection,
}

impl<'a> ScriptContext<'a> {
    pub fn current_buffer_view_handle(&self) -> Option<BufferViewHandle> {
        self.clients
            .get(self.target_client)
            .and_then(|c| c.current_buffer_view_handle())
    }

    pub fn current_buffer_handle(&self) -> Option<BufferHandle> {
        self.current_buffer_view_handle()
            .and_then(|h| self.buffer_views.get(h))
            .map(|v| v.buffer_handle)
    }

    pub fn set_current_buffer_view_handle(&mut self, handle: Option<BufferViewHandle>) {
        if let Some(client) = self.clients.get_mut(self.target_client) {
            client.set_current_buffer_view_handle(handle);
        }
    }
}

const MODULE_SEARCH_PATHS_REGISTRY_KEY: &str = "module_search_paths";
const MODULE_LOADER_REGISTRY_KEY: &str = "module_loader";

pub struct ScriptContextGuard(());

struct ScriptContextRegistryScope<'lua>(&'lua Lua);
impl<'lua> ScriptContextRegistryScope<'lua> {
    pub fn new(lua: &'lua Lua, ctx: &mut ScriptContext) -> ScriptResult<Self> {
        lua.set_named_registry_value("ctx", LuaLightUserData(ctx as *mut ScriptContext as _))?;
        Ok(Self(lua))
    }
}
impl<'lua> Drop for ScriptContextRegistryScope<'lua> {
    fn drop(&mut self) {
        self.0.unset_named_registry_value("ctx").unwrap();
    }
}

pub struct ScriptEngine {
    lua: Lua,
    history: VecDeque<String>,
}

impl ScriptEngine {
    pub fn new() -> Self {
        Self::try_new().unwrap()
    }

    fn try_new() -> ScriptResult<Self> {
        let libs = mlua::StdLib::TABLE
            | mlua::StdLib::STRING
            | mlua::StdLib::UTF8
            | mlua::StdLib::MATH
            | mlua::StdLib::PACKAGE;
        let lua = Lua::new_with(libs)?;

        {
            fn load_module<'lua>(
                lua: &'lua Lua,
                (_module_name, file_path): (LuaString<'lua>, LuaString<'lua>),
            ) -> LuaResult<LuaValue<'lua>> {
                let path = Path::new(file_path.to_str()?);
                eval_file(lua, path)
            }

            fn search_module<'lua>(
                lua: &'lua Lua,
                module_name: LuaString<'lua>,
            ) -> LuaResult<(LuaValue<'lua>, Option<LuaString<'lua>>)> {
                let mut module_path = PathBuf::new();
                let module_name = module_name.to_str()?;
                module_path.reserve(module_name.len());
                for module_part in module_name.split('.') {
                    module_path.push(module_part);
                }
                let module_path = module_path.as_path();

                let mut final_path = PathBuf::new();
                let module_search_paths: LuaTable =
                    lua.named_registry_value(MODULE_SEARCH_PATHS_REGISTRY_KEY)?;
                for module_search_path in module_search_paths.sequence_values::<LuaString>() {
                    let module_search_path = module_search_path?;
                    let module_search_path = Path::new(module_search_path.to_str()?);

                    final_path.clear();
                    final_path.push(module_search_path);
                    final_path.push(module_path);
                    final_path.set_extension("lua");

                    if final_path.exists() {
                        let loader: LuaFunction =
                            lua.named_registry_value(MODULE_LOADER_REGISTRY_KEY)?;
                        let loader = LuaValue::Function(loader);
                        match final_path.to_str() {
                            Some(path) => {
                                let path = lua.create_string(path.as_bytes())?;
                                return Ok((loader, Some(path)));
                            }
                            None => break,
                        }
                    }
                }

                Ok((LuaValue::Nil, None))
            }

            lua.set_named_registry_value(MODULE_SEARCH_PATHS_REGISTRY_KEY, lua.create_table()?)?;
            lua.set_named_registry_value(
                MODULE_LOADER_REGISTRY_KEY,
                lua.create_function(load_module)?,
            )?;

            let searcher = lua.create_function(search_module)?;
            let searchers = lua.create_table()?;
            searchers.set(1, searcher)?;
            let globals = lua.globals();
            let package: LuaTable = globals.get("package")?;
            package.set("searchers", searchers)?;
        }

        let mut this = Self {
            lua,
            history: VecDeque::with_capacity(10),
        };

        script_bindings::bind_all(this.as_ref())?;

        Ok(this)
    }

    pub fn as_ref(&mut self) -> ScriptEngineRef {
        ScriptEngineRef::from_lua(&self.lua)
    }

    pub fn as_ref_with_ctx<F, R>(&mut self, ctx: &mut ScriptContext, scope: F) -> ScriptResult<R>
    where
        F: FnOnce(ScriptEngineRef, &mut ScriptContext, ScriptContextGuard) -> ScriptResult<R>,
    {
        let s = ScriptContextRegistryScope::new(&self.lua, ctx)?;
        let value = scope(
            ScriptEngineRef::from_lua(&self.lua),
            ctx,
            ScriptContextGuard(()),
        )?;
        drop(s);
        Ok(value)
    }

    pub fn add_module_search_path(&mut self, path: &Path) -> ScriptResult<()> {
        let path = path
            .canonicalize()
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let path = if path.is_file() {
            match path.parent() {
                Some(path) => path,
                None => return Ok(()),
            }
        } else {
            path.as_path()
        };

        if let Some(path) = path.to_str() {
            let path = self.lua.create_string(path.as_bytes())?;
            let module_search_paths: LuaTable = self
                .lua
                .named_registry_value(MODULE_SEARCH_PATHS_REGISTRY_KEY)?;
            module_search_paths.set(module_search_paths.len()? + 1, path)?;
        }

        Ok(())
    }

    pub fn eval<'a, F, R>(
        &'a mut self,
        ctx: &mut ScriptContext<'a>,
        source: &str,
        scope: F,
    ) -> ScriptResult<R>
    where
        F: FnOnce(
            ScriptEngineRef<'a>,
            &mut ScriptContext<'a>,
            ScriptContextGuard,
            ScriptValue<'a>,
        ) -> ScriptResult<R>,
    {
        let s = ScriptContextRegistryScope::new(&self.lua, ctx)?;
        let value = self.lua.load(source).set_name(source)?.eval()?;
        let value = scope(
            ScriptEngineRef::from_lua(&self.lua),
            ctx,
            ScriptContextGuard(()),
            value,
        )?;
        drop(s);
        Ok(value)
    }

    pub fn eval_entry_file(&mut self, ctx: &mut ScriptContext, path: &Path) -> ScriptResult<()> {
        self.add_module_search_path(path)?;
        let s = ScriptContextRegistryScope::new(&self.lua, ctx)?;
        let _: LuaValue = eval_file(&self.lua, path)?;
        drop(s);
        Ok(())
    }

    pub fn on_editor_event(
        &mut self,
        ctx: &mut ScriptContext,
        events: &[EditorEvent],
    ) -> ScriptResult<()> {
        let s = ScriptContextRegistryScope::new(&self.lua, ctx)?;
        let engine = ScriptEngineRef::from_lua(&self.lua);
        let mut guard = ScriptContextGuard(());

        macro_rules! call {
            ($callback:ident, $args:expr) => {{
                let callbacks: ScriptArray = match engine.lua.named_registry_value(stringify!($callback)) {
                    Ok(callbacks) => callbacks,
                    Err(_) => continue,
                };
                for callback in callbacks.iter::<ScriptFunction>() {
                    callback?.call(&mut guard, $args.clone())?;
                }
            }};
        }

        for event in events {
            match event {
                EditorEvent::BufferOpen(handle) => call!(buffer_on_open, *handle),
                _ => (),
            }
        }
        drop(s);
        Ok(())
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn history_entry(&self, index: usize) -> &str {
        self.history.get(index).map(String::as_str).unwrap_or("")
    }

    pub fn add_to_history(&mut self, entry: &str) {
        if entry.is_empty() {
            return;
        }

        let mut s = if self.history.len() == self.history.capacity() {
            self.history.pop_front().unwrap()
        } else {
            String::new()
        };

        s.clear();
        s.push_str(entry);
        self.history.push_back(s);
    }
}

fn eval_file<'lua, T>(lua: &'lua Lua, path: &Path) -> LuaResult<T>
where
    T: FromLua<'lua>,
{
    let mut file = File::open(path).map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
    let metadata = file
        .metadata()
        .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
    let mut source = String::with_capacity(metadata.len() as _);
    file.read_to_string(&mut source)
        .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;

    let chunk = lua.load(&source);
    if let Some(name) = path.to_str() {
        chunk.set_name(name)?.eval()
    } else {
        chunk.eval()
    }
}

pub struct ScriptEngineRef<'lua> {
    lua: &'lua Lua,
}

impl<'lua> ScriptEngineRef<'lua> {
    pub fn from_lua(lua: &'lua Lua) -> Self {
        Self { lua }
    }

    pub fn globals_object(&self) -> ScriptObject<'lua> {
        ScriptObject(self.lua.globals())
    }

    pub fn create_string(&self, data: &[u8]) -> ScriptResult<ScriptString<'lua>> {
        self.lua.create_string(data).map(ScriptString)
    }

    pub fn create_object(&self) -> ScriptResult<ScriptObject<'lua>> {
        self.lua.create_table().map(ScriptObject)
    }

    pub fn create_array(&self) -> ScriptResult<ScriptArray<'lua>> {
        self.lua.create_table().map(ScriptArray)
    }

    pub fn create_ctx_function<A, R, F>(&self, func: F) -> ScriptResult<ScriptFunction<'lua>>
    where
        A: FromLuaMulti<'lua>,
        R: ToLuaMulti<'lua>,
        F: 'static
            + Fn(ScriptEngineRef<'lua>, &mut ScriptContext, ScriptContextGuard, A) -> ScriptResult<R>,
    {
        self.lua
            .create_function(move |lua, args| {
                let engine = ScriptEngineRef { lua };
                let ctx: LuaLightUserData = lua.named_registry_value("ctx")?;
                let ctx = unsafe { &mut *(ctx.0 as *mut _) };
                func(engine, ctx, ScriptContextGuard(()), args)
            })
            .map(|f| ScriptFunction(f))
    }

    pub fn save_to_registry<T>(&self, key: &str, value: T) -> ScriptResult<()>
    where
        T: ToLua<'lua>,
    {
        self.lua.set_named_registry_value(key, value)
    }

    pub fn add_to_function_array_in_registry(
        &self,
        key: &str,
        function: ScriptFunction,
    ) -> ScriptResult<()> {
        let function = ScriptValue::Function(function);
        let functions: ScriptResult<ScriptArray> = self.lua.named_registry_value(key);
        match functions {
            Ok(functions) => {
                functions.push(function)?;
            }
            Err(_) => {
                let functions = self.create_array()?;
                functions.push(function)?;
                self.save_to_registry(key, ScriptValue::Array(functions))?;
            }
        }
        Ok(())
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
