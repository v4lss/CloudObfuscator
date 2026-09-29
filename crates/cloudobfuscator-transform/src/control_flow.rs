use crate::context::PassContext;
use swc_common::{Span, SyntaxContext};
use swc_ecma_ast::*;
use swc_ecma_visit::{VisitMut, VisitMutWith};

pub struct Flattener<'a, 'b> {
    context: &'a mut PassContext<'b>,
    top_level: SyntaxContext,
    count: usize,
}

impl Flattener<'_, '_> {
    fn block_is_flattenable(&self, stmts: &[Stmt]) -> bool {
        if stmts.len() < 4 || stmts.len() > self.context.config.max_flattened_statements {
            return false;
        }
        if !stmts.iter().any(contains_branch) {
            return false;
        }
        !stmts.iter().any(unsafe_inside_switch)
    }

    fn flatten(&mut self, span: Span, stmts: &[Stmt]) -> Option<Vec<Stmt>> {
        let state = self.context.fresh_local(span);

        let mut cases: Vec<SwitchCase> = Vec::with_capacity(stmts.len());
        for (index, stmt) in stmts.iter().enumerate() {
            let cons = if index + 1 < stmts.len() {
                vec![stmt.clone(), self.advance(span, &state, (index + 1) as f64)]
            } else {
                vec![stmt.clone(), Stmt::Break(BreakStmt { span, label: None })]
            };
            cases.push(SwitchCase {
                span,
                test: Some(number(span, index as f64)),
                cons,
            });
        }

        let switch_stmt = Stmt::Switch(SwitchStmt {
            span,
            body_ctxt: self.top_level,
            discriminant: Box::new(self.state_ref(span, &state)),
            cases,
        });

        let init = self.state_init(span, &state);

        Some(vec![
            init,
            Stmt::While(WhileStmt {
                span,
                test: Box::new(Expr::Lit(Lit::Bool(Bool { span, value: true }))),
                body: Box::new(Stmt::Block(BlockStmt {
                    span,
                    ctxt: self.top_level,
                    stmts: vec![switch_stmt, Stmt::Break(BreakStmt { span, label: None })],
                })),
            }),
        ])
    }

    fn state_ref(&self, span: Span, state: &Ident) -> Expr {
        Expr::Ident(Ident::new(state.sym.clone(), span, self.top_level))
    }

    fn state_init(&self, span: Span, state: &Ident) -> Stmt {
        Stmt::Decl(Decl::Var(Box::new(VarDecl {
            span,
            kind: VarDeclKind::Var,
            ctxt: self.top_level,
            declare: false,
            decls: vec![VarDeclarator {
                span,
                name: Pat::Ident(BindingIdent {
                    id: Ident::new(state.sym.clone(), span, self.top_level),
                    type_ann: None,
                }),
                init: Some(number(span, 0.0)),
                definite: false,
            }],
        })))
    }

    fn advance(&self, span: Span, state: &Ident, next: f64) -> Stmt {
        Stmt::Expr(ExprStmt {
            span,
            expr: Box::new(Expr::Assign(AssignExpr {
                span,
                op: AssignOp::Assign,
                left: AssignTarget::Simple(SimpleAssignTarget::Ident(BindingIdent {
                    id: Ident::new(state.sym.clone(), span, self.top_level),
                    type_ann: None,
                })),
                right: number(span, next),
            })),
        })
    }
}

impl VisitMut for Flattener<'_, '_> {
    fn visit_mut_function(&mut self, function: &mut Function) {
        function.visit_mut_children_with(self);

        let Some(body) = function.body.as_mut() else {
            return;
        };
        if !self.block_is_flattenable(&body.stmts) {
            return;
        }
        let span = body.span;
        let stmts = std::mem::take(&mut body.stmts);
        match self.flatten(span, &stmts) {
            Some(flattened) => {
                body.stmts = flattened;
                self.count += 1;
            }
            None => body.stmts = stmts,
        }
    }

    fn visit_mut_arrow_expr(&mut self, arrow: &mut ArrowExpr) {
        arrow.visit_mut_children_with(self);
        let ArrowFunctionBody::FunctionBody(body) = &mut *arrow.body else {
            return;
        };
        if !self.block_is_flattenable(&body.stmts) {
            return;
        }
        let span = body.span;
        let stmts = std::mem::take(&mut body.stmts);
        match self.flatten(span, &stmts) {
            Some(flattened) => {
                body.stmts = flattened;
                self.count += 1;
            }
            None => body.stmts = stmts,
        }
    }
}

pub fn run(context: &mut PassContext<'_>, module: &mut Module) {
    if !context.config.flatten_control_flow {
        return;
    }
    let mut flattener = Flattener {
        context,
        top_level: SyntaxContext::empty(),
        count: 0,
    };
    module.visit_mut_with(&mut flattener);
    let count = flattener.count;
    flattener.context.stats.flattened_functions += count;
}

pub fn number(span: Span, value: f64) -> Box<Expr> {
    Box::new(Expr::Lit(Lit::Num(Number {
        span,
        value,
        raw: None,
    })))
}

fn contains_branch(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::If(_)
        | Stmt::Switch(_)
        | Stmt::Try(_)
        | Stmt::For(_)
        | Stmt::ForIn(_)
        | Stmt::ForOf(_)
        | Stmt::While(_)
        | Stmt::DoWhile(_) => true,
        Stmt::Block(block) => block.stmts.iter().any(contains_branch),
        _ => false,
    }
}

fn unsafe_inside_switch(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Decl(Decl::Fn(_)) | Stmt::Decl(Decl::Class(_)) | Stmt::Labeled(_)
    )
}
