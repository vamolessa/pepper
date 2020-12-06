use std::{
    cell::Ref, collections::VecDeque, convert::TryInto, error::Error, fmt, fs::File, io::Read,
    marker::PhantomData, path::Path, sync::Arc,
};

use mlua::prelude::{
    FromLua, FromLuaMulti, Lua, LuaAnyUserData, LuaError, LuaFunction, LuaInteger,
    LuaLightUserData, LuaNumber, LuaResult, LuaString, LuaTable, LuaTableSequence, LuaUserData,
    LuaValue, ToLua, ToLuaMulti,
};

use crate::{
    buffer::{BufferCollection, BufferHandle},
    buffer_view::{BufferViewCollection, BufferViewHandle},
    client::{ClientCollection, TargetClient},
    config::Config,
    editor::{EditorLoop, StatusMessage},
    editor_event::{EditorEvent, EditorEventQueue, EditorEventsIter},
    keymap::KeyMapCollection,
    lsp::{LspClientCollection, LspClientContext},
    mode::Mode,
    picker::Picker,
    register::RegisterCollection,
    script_bindings,
    task::{TaskHandle, TaskManager, TaskResult},
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

#[derive(Clone)]
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

#[derive(Clone)]
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

#[derive(Clone)]
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

#[derive(Clone)]
pub struct ScriptFunction<'lua>(LuaFunction<'lua>);
impl<'lua> ScriptFunction<'lua> {
    pub fn call<A, R>(&self, _: &ScriptContextGuard, args: A) -> ScriptResult<R>
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

pub struct ScriptUserData<'lua, T>(LuaAnyUserData<'lua>, PhantomData<T>)
where
    T: 'static + LuaUserData;
impl<'lua, T> ScriptUserData<'lua, T>
where
    T: 'static + LuaUserData,
{
    pub fn borrow(&self) -> LuaResult<Ref<T>> {
        self.0.borrow()
    }
}
impl<'lua, T> FromLua<'lua> for ScriptUserData<'lua, T>
where
    T: 'static + LuaUserData,
{
    fn from_lua(lua_value: LuaValue<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        if let LuaValue::UserData(d) = lua_value {
            Ok(Self(d, PhantomData))
        } else {
            Err(LuaError::FromLuaConversionError {
                from: lua_value.type_name(),
                to: std::any::type_name::<T>(),
                message: None,
            })
        }
    }
}

#[derive(Clone)]
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

    pub fn display<'a>(&'a self, guard: &'a ScriptContextGuard) -> DisplayScriptValue<'lua, 'a> {
        DisplayScriptValue(self, guard)
    }
}
impl<'lua> FromLua<'lua> for ScriptValue<'lua> {
    fn from_lua(lua_value: LuaValue<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        lua_value_to_script_value(lua_value)
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
fn lua_value_to_script_value<'lua>(value: LuaValue<'lua>) -> LuaResult<ScriptValue<'lua>> {
    match value {
        LuaValue::Nil => Ok(ScriptValue::Nil),
        LuaValue::Boolean(b) => Ok(ScriptValue::Boolean(b)),
        LuaValue::Integer(i) => Ok(ScriptValue::Integer(i)),
        LuaValue::Number(n) => Ok(ScriptValue::Number(n)),
        LuaValue::String(s) => Ok(ScriptValue::String(ScriptString(s))),
        LuaValue::Table(t) => Ok(ScriptValue::Object(ScriptObject(t))),
        LuaValue::Function(f) => Ok(ScriptValue::Function(ScriptFunction(f))),
        _ => Err(LuaError::FromLuaConversionError {
            from: value.type_name(),
            to: std::any::type_name::<ScriptValue>(),
            message: None,
        }),
    }
}

pub struct DisplayScriptValue<'lua, 'value>(&'value ScriptValue<'lua>, &'value ScriptContextGuard);
impl<'lua, 'value> fmt::Display for DisplayScriptValue<'lua, 'value> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fn fmt_recursive(
            value: &ScriptValue,
            f: &mut fmt::Formatter,
            guard: &ScriptContextGuard,
            depth: usize,
        ) -> fmt::Result {
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
                        let table = o.0.clone();
                        match LuaTablePairs::new(table, guard) {
                            Ok(pairs) => {
                                for (key, value) in pairs {
                                    let key = match lua_value_to_script_value(key) {
                                        Ok(key) => key,
                                        Err(_) => continue,
                                    };
                                    let value = match lua_value_to_script_value(value) {
                                        Ok(value) => value,
                                        Err(_) => continue,
                                    };
                                    fmt_recursive(&key, f, guard, depth - 1)?;
                                    f.write_str(":")?;
                                    fmt_recursive(&value, f, guard, depth - 1)?;
                                    f.write_str(",")?;
                                }
                            }
                            Err(table) => {
                                for pair in table.pairs::<ScriptValue, ScriptValue>() {
                                    if let Ok((key, value)) = pair {
                                        fmt_recursive(&key, f, guard, depth - 1)?;
                                        f.write_str(":")?;
                                        fmt_recursive(&value, f, guard, depth - 1)?;
                                        f.write_str(",")?;
                                    }
                                }
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
                                fmt_recursive(&value, f, guard, depth - 1)?;
                            }
                        }
                    }
                    f.write_str("]")?;
                    Ok(())
                }
                ScriptValue::Function(_) => f.write_str("function"),
            }
        }

        fmt_recursive(self.0, f, self.1, 2)
    }
}

struct LuaTablePairs<'lua> {
    table: LuaTable<'lua>,
    key: LuaValue<'lua>,
    next_selector: LuaFunction<'lua>,
}
impl<'lua> LuaTablePairs<'lua> {
    pub fn new(table: LuaTable<'lua>, _: &'lua ScriptContextGuard) -> Result<Self, LuaTable<'lua>> {
        match table
            .get_metatable()
            .and_then(|mt| mt.get("__pairs").ok())
            .and_then(|pairs| {
                let pairs: LuaFunction = pairs;
                pairs.call(table.clone()).ok()
            })
            .map(|(next_selector, table, key)| Self {
                table,
                key,
                next_selector,
            }) {
            Some(s) => Ok(s),
            None => Err(table),
        }
    }
}
impl<'lua> Iterator for LuaTablePairs<'lua> {
    type Item = (LuaValue<'lua>, LuaValue<'lua>);

    fn next(&mut self) -> Option<Self::Item> {
        let key = std::mem::replace(&mut self.key, LuaValue::Nil);
        match self.next_selector.call((self.table.clone(), key)) {
            Ok((LuaValue::Nil, _)) | Err(_) => None,
            Ok((key, value)) => {
                let value: LuaValue = value;
                self.key = key.clone();
                Some((key, value))
            }
        }
    }
}

pub struct ScriptContext<'a> {
    pub target_client: TargetClient,
    pub clients: &'a mut ClientCollection,
    pub editor_loop: EditorLoop,
    pub next_mode: Mode,
    pub edited_buffers: bool,

    pub current_directory: &'a Path,
    pub config: &'a mut Config,

    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub word_database: &'a mut WordDatabase,

    pub registers: &'a mut RegisterCollection,
    pub picker: &'a mut Picker,

    pub status_message: &'a mut StatusMessage,

    pub events: &'a mut EditorEventQueue,
    pub keymaps: &'a mut KeyMapCollection,
    pub tasks: &'a mut TaskManager,
    pub lsp: &'a mut LspClientCollection,
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

    pub fn lsp_context(&mut self) -> (&mut LspClientCollection, LspClientContext) {
        let ctx = LspClientContext {
            current_directory: self.current_directory,
            config: self.config,

            buffers: self.buffers,
            buffer_views: self.buffer_views,
            word_database: self.word_database,

            status_message: self.status_message,
        };

        (self.lsp, ctx)
    }
}

const TASK_CALLBACKS_REGISTRY_KEY: &str = "task_callbacks";
const CURRENT_DIRECTORY_REGISTRY_KEY: &str = "current_path";
struct CurrentDirectory<'a>(&'a Path);

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
        let libs = mlua::StdLib::COROUTINE
            | mlua::StdLib::TABLE
            | mlua::StdLib::IO
            | mlua::StdLib::STRING
            | mlua::StdLib::UTF8
            | mlua::StdLib::MATH
            | mlua::StdLib::PACKAGE;
        let lua = Lua::new_with(libs)?;

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

        if ctx.edited_buffers {
            for buffer in ctx.buffers.iter_mut() {
                buffer.commit_edits();
            }
        }

        drop(s);
        Ok(value)
    }

    pub fn eval_entry_file(&mut self, ctx: &mut ScriptContext, path: &Path) -> ScriptResult<()> {
        let s = ScriptContextRegistryScope::new(&self.lua, ctx)?;
        let _: LuaValue = eval_file(&self.lua, path)?;

        if ctx.edited_buffers {
            for buffer in ctx.buffers.iter_mut() {
                buffer.commit_edits();
            }
        }

        drop(s);
        Ok(())
    }

    pub fn on_editor_event(
        &mut self,
        ctx: &mut ScriptContext,
        events: EditorEventsIter,
    ) -> ScriptResult<()> {
        let s = ScriptContextRegistryScope::new(&self.lua, ctx)?;
        let engine = ScriptEngineRef::from_lua(&self.lua);
        let mut guard = ScriptContextGuard(());

        macro_rules! call {
            ($callback:ident, $args:expr) => {{
                let args = $args;
                if let Ok(callbacks) = engine.lua.named_registry_value(stringify!($callback)) {
                    let callbacks: ScriptArray = callbacks;
                    for callback in callbacks.iter::<ScriptFunction>() {
                        callback?.call(&mut guard, args.clone())?;
                    }
                }
            }};
        }

        for event in events {
            match event {
                EditorEvent::BufferLoad { handle } => {
                    call!(buffer_on_load, *handle)
                }
                EditorEvent::BufferOpen { handle } => {
                    call!(buffer_on_open, *handle)
                }
                EditorEvent::BufferEdit { handle } => {
                    call!(buffer_on_change, *handle)
                }
                EditorEvent::BufferSave { handle, new_path } => {
                    call!(buffer_on_save, (*handle, *new_path))
                }
                EditorEvent::BufferClose { handle } => call!(buffer_on_close, *handle),
            }
        }
        drop(s);
        Ok(())
    }

    pub fn on_task_event(
        &mut self,
        ctx: &mut ScriptContext,
        handle: TaskHandle,
        result: &TaskResult,
    ) -> ScriptResult<()> {
        let s = ScriptContextRegistryScope::new(&self.lua, ctx)?;
        let engine = ScriptEngineRef::from_lua(&self.lua);
        let mut guard = ScriptContextGuard(());

        let callback_index = handle.into_index();

        if let Ok(callbacks) = engine.lua.named_registry_value(TASK_CALLBACKS_REGISTRY_KEY) {
            let callbacks: LuaTable = callbacks;
            let callback: Option<ScriptFunction> = callbacks.get(callback_index)?;
            if let Some(callback) = callback {
                callback.call(&mut guard, result.to_script_value(engine)?)?;
            }
            if let TaskResult::Finished = result {
                callbacks.set(callback_index, LuaValue::Nil)?;
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
    fn try_eval_file<'lua>(lua: &'lua Lua, path: &Path) -> LuaResult<LuaValue<'lua>> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) => {
                return Err(ScriptError::from(format!(
                    "could not open file '{:?}': {}",
                    path, error
                )));
            }
        };

        let metadata = file
            .metadata()
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let mut source = String::with_capacity(metadata.len() as _);
        file.read_to_string(&mut source)
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;

        let chunk = lua.load(&source);
        match path.to_str() {
            Some(name) => chunk.set_name(name)?.eval(),
            None => chunk.eval(),
        }
    }

    let previous_path: LuaValue = lua.named_registry_value(CURRENT_DIRECTORY_REGISTRY_KEY)?;
    let mut current_path = CurrentDirectory(Path::new(""));
    match path.parent() {
        Some(parent) => {
            current_path.0 = parent;
            lua.set_named_registry_value(
                CURRENT_DIRECTORY_REGISTRY_KEY,
                LuaLightUserData(&current_path as *const CurrentDirectory as _),
            )?;
        }
        None => lua.set_named_registry_value(CURRENT_DIRECTORY_REGISTRY_KEY, LuaValue::Nil)?,
    }
    let result = try_eval_file(lua, path);
    drop(current_path);
    lua.set_named_registry_value(CURRENT_DIRECTORY_REGISTRY_KEY, previous_path)?;

    match result {
        Ok(value) => T::from_lua(value, lua),
        Err(error) => Err(error),
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
                let LuaLightUserData(ctx) = lua.named_registry_value("ctx")?;
                let ctx = unsafe { &mut *(ctx as *mut _) };
                func(engine, ctx, ScriptContextGuard(()), args)
            })
            .map(|f| ScriptFunction(f))
    }

    pub fn create_iterator(
        &self,
        keys: &'static [&str],
    ) -> ScriptResult<(ScriptFunction<'lua>, ScriptFunction<'lua>)> {
        let next_function = self
            .lua
            .create_function(move |lua, (table, key)| {
                let table: LuaTable = table;
                let next_index = match key {
                    LuaValue::Nil => Some(0),
                    LuaValue::String(key) => {
                        let key = key.to_str()?;
                        keys.iter()
                            .position(|&k| k == key)
                            .map(|i| i + 1)
                            .filter(|&i| i < keys.len())
                    }
                    _ => return Ok((LuaValue::Nil, LuaValue::Nil)),
                };
                match next_index {
                    Some(i) => {
                        let key = lua.create_string(keys[i].as_bytes())?;
                        let value: LuaValue = table.get(key.clone())?;
                        Ok((LuaValue::String(key), value))
                    }
                    None => Ok((LuaValue::Nil, LuaValue::Nil)),
                }
            })
            .map(|f| ScriptFunction(f))?;

        let pairs_function = self
            .lua
            .create_function(move |_, table| {
                let table: LuaTable = table;
                match table.get_metatable() {
                    Some(meta) => {
                        let next_function: LuaValue = meta.get("__iter")?;
                        Ok((next_function, table, LuaValue::Nil))
                    }
                    None => Ok((LuaValue::Nil, table, LuaValue::Nil)),
                }
            })
            .map(|f| ScriptFunction(f))?;

        Ok((next_function, pairs_function))
    }

    pub fn add_task_callback(
        &self,
        task_handle: TaskHandle,
        callback: ScriptFunction,
    ) -> ScriptResult<()> {
        let index = task_handle.into_index();
        let callback = ScriptValue::Function(callback);

        let callbacks: LuaValue = self.lua.named_registry_value(TASK_CALLBACKS_REGISTRY_KEY)?;
        match callbacks {
            LuaValue::Nil => {
                let callbacks = self.lua.create_table()?;
                callbacks.set(index, callback)?;
                self.lua
                    .set_named_registry_value(TASK_CALLBACKS_REGISTRY_KEY, callbacks.clone())?;
            }
            LuaValue::Table(table) => table.set(index, callback)?,
            _ => (),
        };
        Ok(())
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

    pub fn source(&self, _: &ScriptContextGuard, path: &Path) -> ScriptResult<ScriptValue<'lua>> {
        if path.is_absolute() {
            eval_file(&self.lua, path)
        } else {
            match self.current_source_directory()? {
                Some(parent) => {
                    let mut final_path = parent.to_path_buf();
                    final_path.push(path);
                    eval_file(&self.lua, &final_path)
                }
                None => eval_file(&self.lua, path),
            }
        }
    }

    pub fn current_source_directory<'a>(&self) -> ScriptResult<Option<&'a Path>> {
        match self
            .lua
            .named_registry_value(CURRENT_DIRECTORY_REGISTRY_KEY)?
        {
            LuaValue::Nil => Ok(None),
            LuaValue::LightUserData(LuaLightUserData(current_directory)) => {
                let CurrentDirectory(current_directory) =
                    unsafe { &*(current_directory as *const CurrentDirectory) };
                Ok(Some(current_directory))
            }
            value => Err(ScriptError::from(format!(
                "invalid script source directory '{:?}'",
                value
            ))),
        }
    }
}
