use std::{cell::RefCell, path::PathBuf, rc::Rc};

use mlua::prelude::{Lua, LuaResult, LuaUserData};

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
pub struct ScriptContextRef<'a>(*mut ScriptContext<'a>);
impl<'a> ScriptContextRef<'a> {
    pub fn get(&self) -> &mut ScriptContext<'a> {
        unsafe { &mut *self.0 }
    }
}
impl<'a> LuaUserData for ScriptContextRef<'a> {}

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

        let api = lua.create_table()?;
        api.set("p", lua.create_function(|_, n: String| {
            println!("opaa {}", n);
            Ok(())
        })?)?;
        api.set(
            "print",
            lua.create_function(|_, (_ctx, n): (ScriptContextRef, u64)| {
                println!("aeee {}", n);
                Ok(())
            })?,
        )?;

        lua.globals().set("api", api)?;

        Ok(Self { lua })
    }

    pub fn eval(&mut self, mut ctx: ScriptContext, expression: &str) -> LuaResult<()> {
        self.lua.scope(|scope| {
            let ctx = scope.create_nonstatic_userdata(ScriptContextRef(&mut ctx))?;
            self.lua.globals().set("ctx", ctx)?;
            self.lua.load(expression).exec()?;
            Ok(())
        })
    }

    /*
    pub fn load_entry_file(
        &mut self,
        ctx: ScriptContext,
        path: PathBuf,
    ) -> Result<(), Box<EvalAltResult>> {
        let mut root_path = path.clone();
        root_path.pop();

        self.engine
            .set_module_resolver(Some(FileModuleResolver::new_with_path(root_path)));

        let mut scope = Self::scope(ScriptContextRef::new(ctx));
        let ast = self.engine.compile_file_with_scope(&scope, path)?;
        self.engine.consume_ast_with_scope(&mut scope, &ast)?;

        self.engine
            .set_module_resolver(Option::<FileModuleResolver>::None);
        Ok(())
    }
    */
}
