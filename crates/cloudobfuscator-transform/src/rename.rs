use crate::context::PassContext;
use std::collections::HashMap;
use swc_common::SyntaxContext;
use swc_ecma_ast::*;
use swc_ecma_visit::{VisitMut, VisitMutWith};

pub struct Renamer {
    mapping: HashMap<Id, swc_atoms::Atom>,
    top_level: SyntaxContext,
}

impl Renamer {
    fn rename(&mut self, ident: &mut Ident) -> bool {
        if ident.sym.is_empty() || ident.ctxt == self.top_level {
            return false;
        }
        match self.mapping.get(&ident.to_id()) {
            Some(name) => {
                ident.sym = name.clone();
                true
            }
            None => false,
        }
    }
}

impl VisitMut for Renamer {
    fn visit_mut_ident(&mut self, ident: &mut Ident) {
        self.rename(ident);
    }

    fn visit_mut_module_export_name(&mut self, _name: &mut ModuleExportName) {}

    fn visit_mut_import_named_specifier(&mut self, spec: &mut ImportNamedSpecifier) {
        let original = Ident::new(
            spec.local.sym.clone(),
            spec.local.span,
            SyntaxContext::empty(),
        );
        if self.rename(&mut spec.local) && spec.imported.is_none() {
            spec.imported = Some(ModuleExportName::Ident(original));
        }
    }

    fn visit_mut_import_default_specifier(&mut self, spec: &mut ImportDefaultSpecifier) {
        self.rename(&mut spec.local);
    }

    fn visit_mut_import_star_as_specifier(&mut self, spec: &mut ImportStarAsSpecifier) {
        self.rename(&mut spec.local);
    }

    fn visit_mut_export_named_specifier(&mut self, spec: &mut ExportNamedSpecifier) {
        let ModuleExportName::Ident(orig) = &mut spec.orig else {
            return;
        };
        let original = Ident::new(orig.sym.clone(), orig.span, SyntaxContext::empty());
        if self.rename(orig) && spec.exported.is_none() {
            spec.exported = Some(ModuleExportName::Ident(original));
        }
    }

    fn visit_mut_export_namespace_specifier(&mut self, spec: &mut ExportNamespaceSpecifier) {
        if let ModuleExportName::Ident(name) = &mut spec.name {
            self.rename(name);
        }
    }
}

pub fn run(context: &mut PassContext<'_>, candidates: &[Id], module: &mut Module) {
    if candidates.is_empty() {
        return;
    }

    let mut mapping: HashMap<Id, swc_atoms::Atom> = HashMap::new();
    for id in candidates {
        let name = context.names.next(&mut context.rng);
        mapping.insert(id.clone(), swc_atoms::Atom::from(name));
    }

    let count = mapping.len();
    let mut renamer = Renamer {
        mapping,
        top_level: SyntaxContext::empty(),
    };
    module.visit_mut_with(&mut renamer);
    context.stats.renamed += count;
}
