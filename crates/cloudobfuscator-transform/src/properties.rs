use crate::context::PassContext;
use cloudobfuscator_analysis::{
    plan_property_mangling, PropertyMode, PropertyPlan, ProgramAnalysis,
};
use std::collections::HashMap;
use swc_atoms::Atom;
use swc_ecma_ast::*;
use swc_ecma_visit::{VisitMut, VisitMutWith};

pub struct PropertyMangler {
    map: HashMap<String, String>,
    count: usize,
}

impl PropertyMangler {
    fn rename(&mut self, name: &mut String) {
        if name.is_empty() {
            return;
        }
        if let Some(replacement) = self.map.get(name) {
            *name = replacement.clone();
            self.count += 1;
        }
    }

    fn rename_prop_name(&mut self, name: &mut PropName) {
        if let PropName::Ident(ident) = name {
            let current = ident.sym.to_string();
            let mut renamed = current.clone();
            self.rename(&mut renamed);
            if renamed != current {
                ident.sym = Atom::from(renamed);
            }
        }
    }

    fn rename_member_prop(&mut self, prop: &mut MemberProp) {
        match prop {
            MemberProp::Ident(ident) => {
                let current = ident.sym.to_string();
                let mut renamed = current.clone();
                self.rename(&mut renamed);
                if renamed != current {
                    ident.sym = Atom::from(renamed);
                }
            }
            MemberProp::Computed(computed) => {
                if let Expr::Lit(Lit::Str(node)) = &mut *computed.expr {
                    let current = node.value.to_atom_lossy().to_string();
                    let mut renamed = current.clone();
                    self.rename(&mut renamed);
                    if renamed != current {
                        node.value = swc_atoms::Wtf8Atom::from(renamed);
                    }
                }
            }
            MemberProp::PrivateName(_) => {}
        }
    }
}

impl VisitMut for PropertyMangler {
    fn visit_mut_member_expr(&mut self, member: &mut MemberExpr) {
        member.obj.visit_mut_with(self);
        self.rename_member_prop(&mut member.prop);
    }

    fn visit_mut_opt_chain_expr(&mut self, chain: &mut OptChainExpr) {
        match &mut *chain.base {
            OptChainBase::Member(member) => {
                member.obj.visit_mut_with(self);
                self.rename_member_prop(&mut member.prop);
            }
            OptChainBase::Call(call) => call.visit_mut_with(self),
        }
    }

    fn visit_mut_object_lit(&mut self, literal: &mut ObjectLit) {
        for prop in &mut literal.props {
            match prop {
                PropOrSpread::Prop(prop) => match &mut **prop {
                    Prop::KeyValue(kv) => {
                        self.rename_prop_name(&mut kv.key);
                        kv.value.visit_mut_with(self);
                    }
                    Prop::Shorthand(ident) => {
                        let current = ident.sym.to_string();
                        let mut renamed = current.clone();
                        self.rename(&mut renamed);
                        if renamed != current {
                            self.count += 1;
                            **prop = Prop::KeyValue(KeyValueProp {
                                key: PropName::Ident(IdentName::new(
                                    Atom::from(renamed),
                                    ident.span,
                                )),
                                value: Box::new(Expr::Ident(ident.clone())),
                            });
                        }
                    }
                    Prop::Method(method) => {
                        self.rename_prop_name(&mut method.key);
                        method.function.visit_mut_with(self);
                    }
                    Prop::Getter(getter) => {
                        self.rename_prop_name(&mut getter.key);
                        getter.function.visit_mut_with(self);
                    }
                    Prop::Setter(setter) => {
                        self.rename_prop_name(&mut setter.key);
                        setter.function.visit_mut_with(self);
                    }
                    _ => prop.visit_mut_with(self),
                },
                PropOrSpread::Spread(spread) => spread.expr.visit_mut_with(self),
            }
        }
    }
}

pub fn run(
    context: &mut PassContext<'_>,
    analysis: &ProgramAnalysis,
    module: &mut Module,
    mode: PropertyMode,
) -> PropertyPlan {
    if mode == PropertyMode::Off || !context.config.mangle_properties {
        return PropertyPlan::default();
    }
    if analysis.properties.unknown_access > 0 {
        context
            .stats
            .skipped
            .push("property-mangling: dynamic member access detected".to_string());
        return PropertyPlan::default();
    }

    let plan = {
        let rng = &mut context.rng;
        let names = &mut context.names;
        plan_property_mangling(module, analysis, mode, &mut |index| {
            Atom::from(format!("{}{}", names.next(rng), encode_index(index)))
        })
    };

    if plan.is_empty() {
        return plan;
    }

    let mut mangler = PropertyMangler {
        map: plan
            .map
            .iter()
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect(),
        count: 0,
    };
    module.visit_mut_with(&mut mangler);
    context.stats.mangled_properties += mangler.count;
    plan
}

fn encode_index(mut index: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ$_";
    let mut out = Vec::new();
    loop {
        out.push(ALPHABET[index % ALPHABET.len()]);
        index /= ALPHABET.len();
        if index == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}
