use std::path::PathBuf;

use rhai::{module_resolvers::FileModuleResolver, Engine, EvalAltResult, Scope, AST};

pub struct ScriptEngine {
    engine: Engine,
    entry_ast: Option<AST>,
}

impl ScriptEngine {
    pub fn new() -> Self {
        let engine = Engine::new();
        Self {
            engine,
            entry_ast: None,
        }
    }

    pub fn eval(&mut self, expression: &str) -> Result<i64, Box<EvalAltResult>> {
        let mut scope = Scope::new();

        let ast = self.engine.compile_expression_with_scope(&scope, expression)?;
        let ast = match &self.entry_ast {
            Some(entry_ast) => entry_ast.merge(&ast),
            None => ast,
        };

        self.engine.eval_ast_with_scope(&mut scope, &ast)
    }

    pub fn load_entry_file(&mut self, path: PathBuf) -> Result<(), Box<EvalAltResult>> {
        let mut root_path = path.clone();
        root_path.pop();
        self.engine
            .set_module_resolver(Some(FileModuleResolver::new_with_path(root_path)));

        let mut scope = Scope::new();
        let ast = self.engine.compile_file_with_scope(&scope, path)?;
        self.engine.consume_ast_with_scope(&mut scope, &ast)?;
        self.entry_ast = Some(ast);

        Ok(())
    }
}
