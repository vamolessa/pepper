use std::{
    fs::File,
    io::Read,
    ops::{Deref, DerefMut},
    path::Path,
    sync::Arc,
};

use mlua::prelude::{Lua, LuaError, LuaResult, LuaTable, LuaUserData};

use crate::{
    buffer::BufferCollection,
    buffer_view::{BufferViewCollection, BufferViewHandle},
    config::Config,
    connection::TargetClient,
    editor::ClientTargetMap,
    editor_operation::EditorOperationSerializer,
    keymap::KeyMapCollection,
};

pub struct ScriptContext<'a> {
    pub target_client: TargetClient,
    pub client_target_map: &'a mut ClientTargetMap,
    pub operations: &'a mut EditorOperationSerializer,

    pub config: &'a Config,
    pub keymaps: &'a mut KeyMapCollection,
    pub buffers: &'a mut BufferCollection,
    pub buffer_views: &'a mut BufferViewCollection,
    pub current_buffer_view_handle: &'a mut Option<BufferViewHandle>,
}

#[derive(Clone)]
pub struct ScriptContextRef(*mut ScriptContextRef);
impl ScriptContextRef {
    fn new(ctx: &mut ScriptContext) -> Self {
        Self(ctx as *mut ScriptContext as *mut _)
    }
}
impl Deref for ScriptContextRef {
    type Target = ScriptContext<'static>;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.0 as *const _) }
    }
}
impl DerefMut for ScriptContextRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *(self.0 as *mut _) }
    }
}
impl LuaUserData for ScriptContextRef {}

pub struct ScriptEngine {
    lua: Lua,
}

impl ScriptEngine {
    pub fn new() -> Self {
        Self::try_new().unwrap()
    }

    pub fn try_new() -> LuaResult<Self> {
        let libs = mlua::StdLib::TABLE
            | mlua::StdLib::STRING
            | mlua::StdLib::UTF8
            | mlua::StdLib::MATH
            | mlua::StdLib::PACKAGE;
        let lua = Lua::new_with(libs)?;
        Ok(Self { lua })
    }

    pub fn register_all_functions<'lua, F>(&'lua self, name: &str, func: F) -> LuaResult<()>
    where
        F: FnOnce(&LuaTable<'lua>),
    {
        let functions_table = self.lua.create_table()?;
        func(&functions_table);
        self.lua.globals().set(name, functions_table)?;
        Ok(())
    }

    pub fn eval(&mut self, mut ctx: ScriptContext, source: &str) -> LuaResult<()> {
        self.lua.scope(|scope| {
            let ctx = scope.create_userdata(ScriptContextRef::new(&mut ctx))?;
            self.lua.globals().set("ctx", ctx)?;

            self.lua.load(source).exec()?;
            Ok(())
        })
    }

    pub fn load_entry_file(&mut self, mut ctx: ScriptContext, path: &Path) -> LuaResult<()> {
        let mut file = File::open(path).map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let metadata = file
            .metadata()
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;
        let mut source = String::with_capacity(metadata.len() as _);
        file.read_to_string(&mut source)
            .map_err(|e| LuaError::ExternalError(Arc::new(e)))?;

        self.lua.scope(|scope| {
            let ctx = scope.create_userdata(ScriptContextRef::new(&mut ctx))?;
            self.lua.globals().set("ctx", ctx)?;

            let chunk = self.lua.load(&source);
            let chunk = if let Some(name) = path.to_str() {
                chunk.set_name(name)?
            } else {
                chunk
            };

            chunk.exec()?;
            Ok(())
        })
    }
}
