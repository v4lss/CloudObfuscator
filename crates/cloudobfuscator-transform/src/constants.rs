use crate::context::PassContext;
use swc_common::{Span, Spanned};
use swc_ecma_ast::*;
use swc_ecma_visit::{VisitMut, VisitMutWith};

pub struct NumberObfuscator<'a, 'b> {
    context: &'a mut PassContext<'b>,
    count: usize,
}

impl NumberObfuscator<'_, '_> {
    fn expression(&mut self, span: Span, value: f64) -> Option<Expr> {
        if !value.is_finite() || value == 0.0 || value.fract() != 0.0 {
            return None;
        }
        if value.abs() >= 900_719_925_474_099.0 {
            return None;
        }

        for _ in 0..32 {
            let left = self.context.rng.i32_range(1, 4096);
            let right = self.context.rng.i32_range(1, 4096);
            let (expr, result) = match self.context.rng.below(6) {
                0 => (BinaryOp::Add, (left as f64) + (right as f64)),
                1 => (BinaryOp::Sub, (left as f64) - (right as f64)),
                2 => (BinaryOp::Mul, (left as f64) * (right as f64)),
                3 => (BinaryOp::Div, (left as f64) / (right as f64)),
                4 => (BinaryOp::BitXor, (left ^ right) as f64),
                _ => (BinaryOp::BitOr, (left | right) as f64),
            };
            if result != value || matches!(expr, BinaryOp::Div) {
                continue;
            }
            let left_expr = Expr::Lit(Lit::Num(Number {
                span,
                value: left as f64,
                raw: None,
            }));
            let right_expr = Expr::Lit(Lit::Num(Number {
                span,
                value: right as f64,
                raw: None,
            }));
            return Some(Expr::Bin(BinExpr {
                span,
                op: expr,
                left: Box::new(left_expr),
                right: Box::new(right_expr),
            }));
        }
        None
    }
}

impl VisitMut for NumberObfuscator<'_, '_> {
    fn visit_mut_expr(&mut self, expr: &mut Expr) {
        let candidate = match expr {
            Expr::Lit(Lit::Num(node)) => Some(node.value),
            _ => None,
        };
        if let Some(value) = candidate {
            let span = expr.span();
            if let Some(replacement) = self.expression(span, value) {
                *expr = replacement;
                self.count += 1;
                return;
            }
        }
        expr.visit_mut_children_with(self);
    }
}

pub fn run(context: &mut PassContext<'_>, module: &mut Module) {
    if !context.config.obfuscate_numbers {
        return;
    }
    let mut obfuscator = NumberObfuscator { context, count: 0 };
    module.visit_mut_with(&mut obfuscator);
    let count = obfuscator.count;
    obfuscator.context.stats.obfuscated_numbers += count;
}
