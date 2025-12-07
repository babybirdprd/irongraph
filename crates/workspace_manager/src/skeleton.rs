use std::path::Path;
use syn::{visit_mut::VisitMut, Block, ImplItem, ItemFn, ItemImpl};
use oxc_allocator::Allocator;
use oxc_parser::{Parser, ParserReturn};
use oxc_span::SourceType;
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_ast::ast::FunctionBody;
use oxc_ast_visit::{VisitMut as OxcVisitMut, walk_mut};

struct RustSkeletonVisitor;

impl VisitMut for RustSkeletonVisitor {
    fn visit_block_mut(&mut self, i: &mut Block) {
        if !i.stmts.is_empty() {
            i.stmts = vec![];
        }
    }

    fn visit_item_fn_mut(&mut self, i: &mut ItemFn) {
        self.visit_block_mut(&mut *i.block);
    }

    fn visit_item_impl_mut(&mut self, i: &mut ItemImpl) {
        for item in &mut i.items {
            match item {
                ImplItem::Fn(method) => {
                     self.visit_block_mut(&mut method.block);
                },
                _ => {}
            }
        }
    }
}

struct JsSkeletonVisitor;

impl<'a> OxcVisitMut<'a> for JsSkeletonVisitor {
    fn visit_function_body(&mut self, body: &mut FunctionBody<'a>) {
        body.statements.clear();
        walk_mut::walk_function_body(self, body);
    }
}

pub fn get_skeleton(path: &Path, content: &str) -> Result<String, String> {
    if path.extension().map_or(false, |ext| ext == "rs") {
        let mut syntax = syn::parse_file(content).map_err(|e| format!("Rust parse error: {}", e))?;
        let mut visitor = RustSkeletonVisitor;
        visitor.visit_file_mut(&mut syntax);
        let formatted = prettyplease::unparse(&syntax);
        Ok(formatted)
    } else if path.extension().map_or(false, |ext| ["ts", "tsx", "js", "jsx"].contains(&ext.to_str().unwrap())) {

        let allocator = Allocator::default();
        let source_type = SourceType::from_path(path).unwrap_or_default();

        let ParserReturn { mut program, errors, .. } = Parser::new(&allocator, content, source_type).parse();

        if !errors.is_empty() {
            return Err(format!("JS Parse Error: {:?}", errors[0]));
        }

        let mut visitor = JsSkeletonVisitor;
        visitor.visit_program(&mut program);

        let ret = Codegen::new()
            .with_options(CodegenOptions::default())
            .build(&program);

        Ok(ret.code)
    } else {
        Err("Unsupported file type for skeleton view".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_skeleton() {
        let code = r#"
            fn main() {
                println!("Hello");
                let x = 10;
            }
            struct Foo;
            impl Foo {
                fn bar() {
                    let y = 20;
                }
            }
        "#;
        let skeleton = get_skeleton(Path::new("test.rs"), code).unwrap();
        assert!(skeleton.contains("fn main() {}"));
        assert!(skeleton.contains("impl Foo {"));
        assert!(skeleton.contains("fn bar() {}"));
        assert!(!skeleton.contains("println"));
        assert!(!skeleton.contains("let x"));
        assert!(!skeleton.contains("let y"));
    }

    #[test]
    fn test_ts_skeleton() {
        let code = r#"
            function main() {
                console.log("Hello");
            }
            class Foo {
                bar() {
                    const y = 20;
                }
            }
        "#;
        let skeleton = get_skeleton(Path::new("test.ts"), code).unwrap();
        // Oxc printing format might vary slightly
        assert!(skeleton.contains("function main() {"));
        assert!(skeleton.contains("class Foo {"));
        assert!(skeleton.contains("bar() {"));
        assert!(!skeleton.contains("console.log"));
        assert!(!skeleton.contains("const y"));
    }
}
