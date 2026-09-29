use crate::collect::{BindingKind, ProgramAnalysis};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast::Id;
use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitWith};
use swc_atoms::Atom;

const RESERVED_OBJECT_KEYS: &[&str] = &[
    "constructor",
    "__proto__",
    "prototype",
    "toString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
    "then",
    "length",
    "name",
    "call",
    "apply",
    "bind",
    "caller",
    "arguments",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PropertyMode {
    Off,
    Safe,
    Aggressive,
}

impl PropertyMode {
    pub fn from_str(value: &str) -> PropertyMode {
        match value {
            "off" | "none" => PropertyMode::Off,
            "aggressive" => PropertyMode::Aggressive,
            _ => PropertyMode::Safe,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct PropertyPlan {
    pub map: HashMap<Atom, Atom>,
    pub objects: usize,
    pub keys: usize,
    pub rejected: usize,
}

impl PropertyPlan {
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

struct Candidate {
    ident: Ident,
    keys: Vec<String>,
}

#[derive(Default)]
struct Usage {
    safe: bool,
    keys: HashSet<String>,
}

struct UsageCollector {
    targets: HashMap<Id, Usage>,
    active: Vec<Id>,
}

impl Visit for UsageCollector {
    fn visit_binding_ident(&mut self, ident: &BindingIdent) {
        if self.targets.contains_key(&ident.id.to_id()) {
            return;
        }
        ident.id.visit_with(self);
    }

    fn visit_ident(&mut self, ident: &Ident) {
        if ident.sym.is_empty() {
            return;
        }
        if self.targets.contains_key(&ident.to_id()) {
            if let Some(usage) = self.targets.get_mut(&ident.to_id()) {
                usage.safe = false;
            }
        }
    }

    fn visit_member_expr(&mut self, member: &MemberExpr) {
        if let Expr::Ident(obj) = &*member.obj {
            let id = obj.to_id();
            if self.targets.contains_key(&id) {
                match &member.prop {
                    MemberProp::Ident(name) => {
                        if let Some(usage) = self.targets.get_mut(&id) {
                            usage.keys.insert(name.sym.to_string());
                        }
                        return;
                    }
                    MemberProp::Computed(computed) => {
                        if let Expr::Lit(Lit::Str(s)) = &*computed.expr {
                            let key = s.value.to_atom_lossy().to_string();
                            if let Some(usage) = self.targets.get_mut(&id) {
                                usage.keys.insert(key);
                            }
                            return;
                        }
                        if let Some(usage) = self.targets.get_mut(&id) {
                            usage.safe = false;
                        }
                        return;
                    }
                    MemberProp::PrivateName(_) => {
                        return;
                    }
                }
            }
        }
        member.obj.visit_with(self);
        if let MemberProp::Computed(computed) = &member.prop {
            computed.expr.visit_with(self);
        }
    }
}

struct CandidateFinder {
    candidates: Vec<Candidate>,
}

impl Visit for CandidateFinder {
    fn visit_var_decl(&mut self, decl: &VarDecl) {
        for declarator in &decl.decls {
            if let (Pat::Ident(binding), Some(Expr::Object(literal))) =
                (&declarator.name, declarator.init.as_deref())
            {
                if let Some(keys) = manglable_keys(literal) {
                    for prop in &literal.props {
                        if let PropOrSpread::Prop(prop) = prop {
                            if let Prop::KeyValue(kv) = &**prop {
                                kv.value.visit_with(self);
                            }
                        }
                    }
                    self.candidates.push(Candidate {
                        ident: binding.id.clone(),
                        keys,
                    });
                    continue;
                }
            }
            declarator.name.visit_with(self);
            if let Some(init) = &declarator.init {
                init.visit_with(self);
            }
        }
    }
}

fn manglable_keys(literal: &ObjectLit) -> Option<Vec<String>> {
    if literal.props.is_empty() {
        return None;
    }
    let mut keys: Vec<String> = Vec::new();
    for prop in &literal.props {
        match prop {
            PropOrSpread::Prop(prop) => match &**prop {
                Prop::KeyValue(kv) => match &kv.key {
                    PropName::Ident(name) => {
                        let sym = name.sym.to_string();
                        if sym.is_empty() || RESERVED_OBJECT_KEYS.contains(&sym.as_str()) {
                            return None;
                        }
                        keys.push(sym);
                    }
                    _ => return None,
                },
                _ => return None,
            },
            PropOrSpread::Spread(_) => return None,
        }
    }
    let unique: HashSet<&String> = keys.iter().collect();
    if unique.len() != keys.len() {
        return None;
    }
    Some(keys)
}

pub fn plan_property_mangling(
    module: &Module,
    analysis: &ProgramAnalysis,
    mode: PropertyMode,
    allocator: &mut dyn FnMut(usize) -> Atom,
) -> PropertyPlan {
    let mut plan = PropertyPlan::default();
    if mode == PropertyMode::Off {
        return plan;
    }

    let mut finder = CandidateFinder {
        candidates: Vec::new(),
    };
    module.visit_with(&mut finder);

    let mut collector = UsageCollector {
        targets: HashMap::new(),
        active: Vec::new(),
    };
    for candidate in &finder.candidates {
        let id = candidate.ident.to_id();
        let Some(info) = analysis.bindings.get(&id) else {
            continue;
        };
        if info.exported || info.mutated {
            continue;
        }
        if info.kind != BindingKind::Const && info.kind != BindingKind::Let {
            continue;
        }
        collector.targets.insert(id, Usage::default());
    }
    if collector.targets.is_empty() {
        return plan;
    }
    module.visit_with(&mut collector);
    let _ = &collector.active;

    for candidate in finder.candidates {
        let id = candidate.ident.to_id();
        let Some(usage) = collector.targets.get(&id) else {
            plan.rejected += 1;
            continue;
        };
        if !usage.safe {
            plan.rejected += 1;
            continue;
        }
        let declared: HashSet<&String> = candidate.keys.iter().collect();
        if !usage.keys.iter().all(|key| declared.contains(key)) {
            plan.rejected += 1;
            continue;
        }
        plan.objects += 1;
        for key in candidate.keys {
            let key_atom = Atom::from(key.as_str());
            if plan.map.contains_key(&key_atom) {
                continue;
            }
            let fresh = allocator(plan.keys);
            plan.map.insert(key_atom, fresh);
            plan.keys += 1;
        }
    }

    if mode == PropertyMode::Safe {
        plan.rejected = 0;
    }

    plan
}
