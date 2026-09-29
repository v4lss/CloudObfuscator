use crate::context::PassContext;
use swc_atoms::Atom;
use swc_common::{Spanned, SyntaxContext};
use swc_ecma_ast::*;
use swc_ecma_visit::{VisitMut, VisitMutWith};

const ENCODABLE_LENGTH: usize = 1_048_576;

pub struct StringExtractor<'a, 'b> {
    context: &'a mut PassContext<'b>,
    top_level: SyntaxContext,
    extracted: usize,
    protected: usize,
    in_object_key: usize,
    in_import_export: usize,
    in_template: usize,
    in_directive: usize,
    in_tagged_template: usize,
}

impl StringExtractor<'_, '_> {
    fn encode(&mut self, expr: &mut Expr) {
        let value = match expr {
            Expr::Lit(Lit::Str(node)) => node.value.to_atom_lossy().to_string(),
            _ => return,
        };
        if value.is_empty() || value.len() > ENCODABLE_LENGTH {
            self.protected += 1;
            return;
        }

        let Some(accessor) = self.context.strings.accessor(&value) else {
            return;
        };
        let Some((decoder, argument)) = accessor.split_once('(') else {
            self.protected += 1;
            return;
        };
        let Ok(slot) = argument.trim_end_matches(')').parse::<f64>() else {
            self.protected += 1;
            return;
        };

        let span = expr.span();
        let callee = Expr::Ident(Ident::new(Atom::from(decoder), span, self.top_level));
        *expr = Expr::Call(CallExpr {
            span,
            ctxt: SyntaxContext::empty(),
            callee: Callee::Expr(Box::new(callee)),
            args: vec![ExprOrSpread {
                spread: None,
                expr: Box::new(Expr::Lit(Lit::Num(Number {
                    span,
                    value: slot,
                    raw: None,
                }))),
            }],
            type_args: None,
        });
        self.extracted += 1;
    }

    fn rename_property_key(&mut self, key: &mut PropName) {
        if let PropName::Ident(ident) = key {
            let mangled = self
                .context
                .property_map
                .get(&ident.sym.to_string())
                .cloned();
            if let Some(mangled) = mangled {
                ident.sym = Atom::from(mangled);
            }
        }
    }
}

impl VisitMut for StringExtractor<'_, '_> {
    fn visit_mut_expr(&mut self, expr: &mut Expr) {
        let blocked = self.in_object_key > 0
            || self.in_import_export > 0
            || self.in_template > 0
            || self.in_tagged_template > 0
            || self.in_directive > 0;
        if blocked {
            if matches!(expr, Expr::Lit(Lit::Str(_))) {
                self.protected += 1;
            }
            expr.visit_mut_children_with(self);
            return;
        }
        self.encode(expr);
        expr.visit_mut_children_with(self);
    }

    fn visit_mut_expr_stmt(&mut self, stmt: &mut ExprStmt) {
        let is_directive = matches!(&*stmt.expr, Expr::Lit(Lit::Str(_)));
        if is_directive {
            self.in_directive += 1;
        }
        stmt.visit_mut_children_with(self);
        if is_directive {
            self.in_directive -= 1;
        }
    }

    fn visit_mut_object_lit(&mut self, literal: &mut ObjectLit) {
        for prop in &mut literal.props {
            match prop {
                PropOrSpread::Prop(prop) => match &mut **prop {
                    Prop::KeyValue(kv) => {
                        self.rename_property_key(&mut kv.key);
                        self.in_object_key += 1;
                        kv.key.visit_mut_with(self);
                        self.in_object_key -= 1;
                        kv.value.visit_mut_with(self);
                    }
                    Prop::Shorthand(ident) => self.protected += usize::from(!ident.sym.is_empty()),
                    Prop::Method(method) => {
                        self.in_object_key += 1;
                        method.key.visit_mut_with(self);
                        self.in_object_key -= 1;
                        method.function.visit_mut_with(self);
                    }
                    Prop::Getter(getter) => {
                        self.in_object_key += 1;
                        getter.key.visit_mut_with(self);
                        self.in_object_key -= 1;
                        getter.function.visit_mut_with(self);
                    }
                    Prop::Setter(setter) => {
                        self.in_object_key += 1;
                        setter.key.visit_mut_with(self);
                        self.in_object_key -= 1;
                        setter.function.visit_mut_with(self);
                    }
                    other => other.visit_mut_with(self),
                },
                PropOrSpread::Spread(spread) => spread.expr.visit_mut_with(self),
            }
        }
    }

    fn visit_mut_class(&mut self, class: &mut Class) {
        class.decorators.visit_mut_with(self);
        class.super_class.visit_mut_with(self);
        for member in &mut class.body {
            match member {
                ClassMember::Method(method) => {
                    self.in_object_key += 1;
                    method.key.visit_mut_with(self);
                    self.in_object_key -= 1;
                    method.function.visit_mut_with(self);
                }
                ClassMember::Constructor(ctor) => ctor.body.visit_mut_with(self),
                ClassMember::PrivateMethod(method) => {
                    method.function.visit_mut_with(self);
                }
                ClassMember::ClassProp(prop) => {
                    self.in_object_key += 1;
                    prop.key.visit_mut_with(self);
                    self.in_object_key -= 1;
                    prop.value.visit_mut_with(self);
                }
                ClassMember::PrivateProp(prop) => prop.value.visit_mut_with(self),
                ClassMember::AutoAccessor(accessor) => {
                    self.in_object_key += 1;
                    accessor.key.visit_mut_with(self);
                    self.in_object_key -= 1;
                    accessor.value.visit_mut_with(self);
                }
                ClassMember::StaticBlock(block) => block.visit_mut_with(self),
                other => other.visit_mut_with(self),
            }
        }
    }

    fn visit_mut_member_expr(&mut self, member: &mut MemberExpr) {
        if let MemberProp::Computed(computed) = &mut member.prop {
            self.in_object_key += 1;
            computed.expr.visit_mut_with(self);
            self.in_object_key -= 1;
        } else {
            member.visit_mut_children_with(self);
        }
    }

    fn visit_mut_opt_chain_expr(&mut self, chain: &mut OptChainExpr) {
        if let OptChainBase::Member(member) = &mut *chain.base {
            member.obj.visit_mut_with(self);
            if let MemberProp::Computed(computed) = &mut member.prop {
                self.in_object_key += 1;
                computed.expr.visit_mut_with(self);
                self.in_object_key -= 1;
            }
        } else {
            chain.visit_mut_children_with(self);
        }
    }

    fn visit_mut_import_decl(&mut self, decl: &mut ImportDecl) {
        self.in_import_export += 1;
        decl.src.visit_mut_with(self);
        self.in_import_export -= 1;
        decl.specifiers.visit_mut_with(self);
        self.in_import_export += 1;
        if let Some(with) = &mut decl.with {
            with.visit_mut_with(self);
        }
        self.in_import_export -= 1;
    }

    fn visit_mut_export_all(&mut self, node: &mut ExportAll) {
        self.in_import_export += 1;
        node.src.visit_mut_with(self);
        if let Some(with) = &mut node.with {
            with.visit_mut_with(self);
        }
        self.in_import_export -= 1;
    }

    fn visit_mut_named_export(&mut self, node: &mut NamedExport) {
        if let Some(src) = &mut node.src {
            self.in_import_export += 1;
            src.visit_mut_with(self);
            self.in_import_export -= 1;
        }
        node.specifiers.visit_mut_with(self);
    }

    fn visit_mut_call_expr(&mut self, call: &mut CallExpr) {
        if matches!(call.callee, Callee::Import(_)) {
            self.in_import_export += 1;
            call.visit_mut_children_with(self);
            self.in_import_export -= 1;
            return;
        }
        call.visit_mut_children_with(self);
    }

    fn visit_mut_tpl(&mut self, tpl: &mut Tpl) {
        tpl.exprs.visit_mut_with(self);
        for quasi in &mut tpl.quasis {
            self.in_template += 1;
            quasi.visit_mut_with(self);
            self.in_template -= 1;
        }
    }

    fn visit_mut_tagged_tpl(&mut self, tagged: &mut TaggedTpl) {
        tagged.tag.visit_mut_with(self);
        self.in_tagged_template += 1;
        tagged.type_params.visit_mut_with(self);
        tagged.tpl.visit_mut_with(self);
        self.in_tagged_template -= 1;
    }
}

pub fn run(context: &mut PassContext<'_>, module: &mut Module) {
    if context.strings.is_empty() {
        return;
    }
    let mut extractor = StringExtractor {
        context,
        top_level: SyntaxContext::empty(),
        extracted: 0,
        protected: 0,
        in_object_key: 0,
        in_import_export: 0,
        in_template: 0,
        in_directive: 0,
        in_tagged_template: 0,
    };
    module.visit_mut_with(&mut extractor);
    let context = extractor.context;
    context.stats.extracted_strings += extractor.extracted;
    context.stats.protected_strings += extractor.protected;
}
