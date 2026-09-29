use crate::context::PassContext;
use swc_common::{Span, SyntaxContext};
use swc_ecma_ast::*;
use swc_ecma_visit::{VisitMut, VisitMutWith};

pub struct OpaqueInserter<'a, 'b> {
    context: &'a mut PassContext<'b>,
    top_level: SyntaxContext,
    count: usize,
}

impl OpaqueInserter<'_, '_> {
    fn guard(&mut self, span: Span) -> Option<Stmt> {
        let test = self.always_true(span)?;
        let sink = self.context.fresh_local(span);
        let init = Stmt::Decl(Decl::Var(Box::new(VarDecl {
            span,
            kind: VarDeclKind::Var,
            ctxt: self.top_level,
            declare: false,
            decls: vec![VarDeclarator {
                span,
                name: Pat::Ident(BindingIdent {
                    id: Ident::new(sink.sym.clone(), span, self.top_level),
                    type_ann: None,
                }),
                init: Some(crate::control_flow::number(span, 0.0)),
                definite: false,
            }],
        })));
        let dead = Stmt::Expr(ExprStmt {
            span,
            expr: Box::new(Expr::Assign(AssignExpr {
                span,
                op: AssignOp::Assign,
                left: AssignTarget::Simple(SimpleAssignTarget::Ident(BindingIdent {
                    id: Ident::new(sink.sym.clone(), span, self.top_level),
                    type_ann: None,
                })),
                right: crate::control_flow::number(span, self.context.num()),
            })),
        });
        Some(Stmt::Block(BlockStmt {
            span,
            ctxt: self.top_level,
            stmts: vec![
                init,
                Stmt::If(IfStmt {
                    span,
                    test: Box::new(test),
                    cons: Box::new(Stmt::Empty(EmptyStmt { span })),
                    alt: Some(Box::new(dead)),
                }),
            ],
        }))
    }

    fn always_true(&mut self, span: Span) -> Option<Expr> {
        for _ in 0..32 {
            let a = self.context.rng.i32_range(1, 512);
            let b = self.context.rng.i32_range(1, 512);
            let c = self.context.rng.i32_range(1, 512);
            let value = ((a ^ b) as f64) + ((c * 7) as f64);
            if value == 0.0 {
                continue;
            }
            let inner = Expr::Bin(BinExpr {
                span,
                op: BinaryOp::BitXor,
                left: crate::control_flow::number(span, a as f64),
                right: crate::control_flow::number(span, b as f64),
            });
            let tail = Expr::Bin(BinExpr {
                span,
                op: BinaryOp::Add,
                left: Box::new(inner),
                right: crate::control_flow::number(span, (c * 7) as f64),
            });
            return Some(tail);
        }
        None
    }

    fn sprinkle(&mut self, body: &mut FunctionBody) -> usize {
        let span = body.span;
        let stmts = std::mem::take(&mut body.stmts);
        let mut rebuilt = Vec::with_capacity(stmts.len() * 2);
        let total = stmts.len();
        let mut inserted = 0;
        if total < 3 {
            body.stmts = stmts;
            return 0;
        }
        for (index, stmt) in stmts.into_iter().enumerate() {
            let last = index + 1 == total;
            if !last && self.context.rng.chance(38) {
                if let Some(guard) = self.guard(span) {
                    rebuilt.push(guard);
                    inserted += 1;
                }
            }
            rebuilt.push(stmt);
        }
        body.stmts = rebuilt;
        inserted
    }
}

impl VisitMut for OpaqueInserter<'_, '_> {
    fn visit_mut_function(&mut self, function: &mut Function) {
        function.visit_mut_children_with(self);
        if let Some(body) = function.body.as_mut() {
            self.count += self.sprinkle(body);
        }
    }

    fn visit_mut_arrow_expr(&mut self, arrow: &mut ArrowExpr) {
        arrow.visit_mut_children_with(self);
        if let ArrowFunctionBody::FunctionBody(body) = &mut *arrow.body {
            self.count += self.sprinkle(body);
        }
    }
}

pub fn run(context: &mut PassContext<'_>, module: &mut Module) {
    if !context.config.opaque_predicates {
        return;
    }
    let mut inserter = OpaqueInserter {
        context,
        top_level: SyntaxContext::empty(),
        count: 0,
    };
    module.visit_mut_with(&mut inserter);
    inserter.context.stats.opaque_predicates += inserter.count;
}
