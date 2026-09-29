use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitWith};
use swc_atoms::Atom;

pub fn is_pure_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(_) | Expr::Arrow(_) | Expr::Fn(_) | Expr::Class(_) | Expr::Ident(_) => true,
        Expr::Paren(p) => is_pure_expr(&p.expr),
        Expr::Tpl(t) => t.exprs.iter().all(|e| is_pure_expr(e)),
        Expr::Array(a) => a
            .elems
            .iter()
            .flatten()
            .all(|e| is_pure_expr(&e.expr)),
        Expr::Object(o) => o.props.iter().all(|p| match p {
            PropOrSpread::Prop(prop) => prop_is_pure(prop),
            PropOrSpread::Spread(_) => false,
        }),
        Expr::Unary(u) => {
            matches!(u.op, UnaryOp::TypeOf | UnaryOp::Void | UnaryOp::Delete)
                && is_pure_expr(&u.arg)
        }
        Expr::Bin(b) => is_pure_expr(&b.left) && is_pure_expr(&b.right),
        Expr::Cond(c) => is_pure_expr(&c.test) && is_pure_expr(&c.cons) && is_pure_expr(&c.alt),
        Expr::Seq(s) => s.exprs.iter().all(|e| is_pure_expr(e)),
        _ => false,
    }
}

fn prop_is_pure(prop: &Prop) -> bool {
    match prop {
        Prop::Shorthand(_) => true,
        Prop::KeyValue(kv) => is_pure_expr(&kv.value),
        Prop::Assign(a) => is_pure_expr(&a.value),
        Prop::Getter(g) => g.function.body.is_none(),
        Prop::Setter(s) => s.function.body.is_none(),
        Prop::Method(m) => m.function.body.is_none(),
    }
}

pub fn has_side_effect(expr: &Expr) -> bool {
    !is_pure_expr(expr)
}

pub fn is_pure_pat(pat: &Pat) -> bool {
    matches!(
        pat,
        Pat::Ident(_) | Pat::Rest(_) | Pat::Array(_) | Pat::Object(_) | Pat::Assign(_)
    )
}

pub fn is_simple_target(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(_) | Expr::Lit(_) => true,
        Expr::Member(m) => {
            matches!(m.prop, MemberProp::Ident(_) | MemberProp::Computed(_)) && is_simple_target(&m.obj)
        }
        Expr::Paren(p) => is_simple_target(&p.expr),
        _ => false,
    }
}

pub fn pat_contains_ident(pat: &Pat) -> bool {
    struct Finder {
        found: bool,
    }
    impl Visit for Finder {
        fn visit_ident(&mut self, ident: &Ident) {
            if !ident.sym.is_empty() {
                self.found = true;
            }
        }
    }
    let mut finder = Finder { found: false };
    pat.visit_with(&mut finder);
    finder.found
}

pub fn expr_contains_ident(expr: &Expr, sym: &Atom) -> bool {
    struct Finder<'a> {
        sym: &'a Atom,
        found: bool,
    }
    impl Visit for Finder<'_> {
        fn visit_ident(&mut self, ident: &Ident) {
            if &ident.sym == self.sym {
                self.found = true;
            }
        }
    }
    let mut finder = Finder { sym, found: false };
    expr.visit_with(&mut finder);
    finder.found
}

pub fn body_stmts(body: &ArrowFunctionBody) -> &[Stmt] {
    match body {
        ArrowFunctionBody::FunctionBody(body) => &body.stmts,
        ArrowFunctionBody::Expr(_) => &[],
    }
}
