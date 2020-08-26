use std::path::PathBuf;

use rhai::{
    def_package,
    module_resolvers::FileModuleResolver,
    packages::{EvalPackage, Package},
    Engine, EvalAltResult, ImmutableString, Scope,
};

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

pub struct ScriptEngine {
    engine: Engine,
}

impl ScriptEngine {
    pub fn new() -> Self {
        let mut engine = Engine::new();
        engine.load_package(EvalPackage::new().get());
        engine.load_package(ApiPackage::new().get());

        Self { engine }
    }

    pub fn eval(
        &mut self,
        ctx: ScriptContext,
        expression: &str,
    ) -> Result<(), Box<EvalAltResult>> {
        let mut scope = Scope::new();
        self.engine
            .eval_expression_with_scope(&mut scope, expression)
    }

    pub fn load_entry_file(&mut self, path: PathBuf) -> Result<(), Box<EvalAltResult>> {
        let mut root_path = path.clone();
        root_path.pop();

        self.engine
            .set_module_resolver(Some(FileModuleResolver::new_with_path(root_path)));

        let mut scope = Scope::new();
        let ast = self.engine.compile_file_with_scope(&scope, path)?;
        self.engine.consume_ast_with_scope(&mut scope, &ast)?;

        self.engine
            .set_module_resolver(Option::<FileModuleResolver>::None);
        Ok(())
    }
}

def_package!(rhai:ApiPackage:"pepper api", module, {
    module.set_fn_1("my_print", |s: ImmutableString| {
        println!("hello {}", s);
        Ok(())
    });
});
