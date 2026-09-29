use std::collections::{HashMap, HashSet};
use swc_atoms::Atom;
use swc_common::{Mark, Span, SyntaxContext};
use swc_ecma_ast::Id;
use swc_ecma_ast::*;
use swc_ecma_visit::{Visit, VisitWith};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    Var,
    Let,
    Const,
    Function,
    Class,
    Param,
    Import,
    Catch,
    Other,
}

#[derive(Debug, Clone)]
pub struct BindingInfo {
    pub kind: BindingKind,
    pub name: String,
    pub references: usize,
    pub top_level: bool,
    pub exported: bool,
    pub mutated: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StringSite {
    pub span: Span,
    pub value: String,
    pub protected: bool,
}

#[derive(Debug, Clone)]
pub struct NumberSite {
    pub span: Span,
    pub value: f64,
}

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    pub name: Option<String>,
    pub span: Span,
    pub params: usize,
    pub statements: usize,
    pub branches: usize,
    pub is_async: bool,
    pub is_generator: bool,
    pub top_level: bool,
    pub flattenable: bool,
    pub is_arrow: bool,
}

#[derive(Default)]
pub struct PropertyIndex {
    pub member_reads: HashMap<String, usize>,
    pub member_writes: HashMap<String, usize>,
    pub object_keys: HashMap<String, usize>,
    pub computed_access: HashMap<String, usize>,
    pub unknown_access: usize,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct RiskItem {
    pub kind: String,
    pub count: usize,
}

#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct AnalysisStats {
    pub bytes: usize,
    pub functions: usize,
    pub arrow_functions: usize,
    pub classes: usize,
    pub identifiers: usize,
    pub renameable_identifiers: usize,
    pub global_references: usize,
    pub strings: usize,
    pub extractable_strings: usize,
    pub numeric_literals: usize,
    pub dynamic_imports: usize,
    pub exports: usize,
    pub distinct_properties: usize,
    pub flatten_candidates: usize,
    pub avg_function_size: f64,
    pub risks: Vec<RiskItem>,
}

#[derive(Default)]
pub struct ProgramAnalysis {
    pub bindings: HashMap<Id, BindingInfo>,
    pub globals: HashSet<Atom>,
    pub pinned: HashSet<Id>,
    pub strings: Vec<StringSite>,
    pub numbers: Vec<NumberSite>,
    pub functions: Vec<FunctionInfo>,
    pub properties: PropertyIndex,
    pub stats: AnalysisStats,
    pub has_eval: bool,
    pub has_with: bool,
    pub has_debugger: bool,
    pub has_new_function: bool,
}

impl ProgramAnalysis {
    pub fn collect(
        module: &Module,
        unresolved_mark: Mark,
        source_bytes: usize,
    ) -> ProgramAnalysis {
        let mut analysis = ProgramAnalysis::default();
        let mut collector = Collector {
            analysis: &mut analysis,
            excluded: HashSet::new(),
            unresolved: unresolved_mark,
            depth: 0,
        };
        collector.run(module);
        analysis.finalize(source_bytes);
        analysis
    }

    pub fn is_renameable(&self, id: &Id) -> bool {
        !self.pinned.contains(id) && self.bindings.contains_key(id)
    }

    pub fn rename_candidates(&self) -> Vec<Id> {
        let mut ids: Vec<Id> = self
            .bindings
            .iter()
            .filter(|(id, _)| self.is_renameable(id))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.as_u32().cmp(&b.1.as_u32()))
        });
        ids
    }

    fn finalize(&mut self, source_bytes: usize) {
        self.stats.bytes = source_bytes;
        self.stats.identifiers = self.bindings.len();
        self.stats.renameable_identifiers = self
            .bindings
            .keys()
            .filter(|id| self.is_renameable(id))
            .count();
        self.stats.global_references = self.globals.len();
        self.stats.strings = self.strings.len();
        self.stats.extractable_strings = self.strings.iter().filter(|s| !s.protected).count();
        self.stats.numeric_literals = self.numbers.len();
        self.stats.functions = self.functions.iter().filter(|f| !f.is_arrow).count();
        self.stats.arrow_functions = self.functions.iter().filter(|f| f.is_arrow).count();
        self.stats.flatten_candidates = self.functions.iter().filter(|f| f.flattenable).count();
        self.stats.classes = self
            .bindings
            .values()
            .filter(|b| b.kind == BindingKind::Class)
            .count();
        self.stats.avg_function_size = if self.functions.is_empty() {
            0.0
        } else {
            let total: usize = self.functions.iter().map(|f| f.statements).sum();
            total as f64 / self.functions.len() as f64
        };

        let mut distinct: HashSet<&String> = HashSet::new();
        distinct.extend(self.properties.member_reads.keys());
        distinct.extend(self.properties.member_writes.keys());
        distinct.extend(self.properties.object_keys.keys());
        distinct.extend(self.properties.computed_access.keys());
        self.stats.distinct_properties = distinct.len();

        let mut risks = Vec::new();
        let mut push = |kind: &str, count: usize| {
            if count > 0 {
                risks.push(RiskItem {
                    kind: kind.to_string(),
                    count,
                });
            }
        };
        push("eval", self.has_eval as usize);
        push("with", self.has_with as usize);
        push("debugger", self.has_debugger as usize);
        push("new Function", self.has_new_function as usize);
        push("dynamic property access", self.properties.unknown_access);
        push("computed property keys", self.properties.computed_access.len());
        self.stats.risks = risks;
    }
}

struct Collector<'a> {
    analysis: &'a mut ProgramAnalysis,
    excluded: HashSet<(u32, u32)>,
    unresolved: Mark,
    depth: usize,
}

impl<'a> Collector<'a> {
    fn run(&mut self, module: &Module) {
        for span in collect_directive_spans(module) {
            self.exclude(span);
        }
        for item in &module.body {
            match item {
                ModuleItem::ModuleDecl(ModuleDecl::Import(import)) => self.exclude(import.src.span),
                ModuleItem::ModuleDecl(ModuleDecl::ExportAll(all)) => self.exclude(all.src.span),
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(named)) => {
                    if let Some(src) = &named.src {
                        self.exclude(src.span);
                    }
                }
                _ => {}
            }
        }
        module.visit_with(self);
    }

    fn exclude(&mut self, span: Span) {
        self.excluded.insert((span.lo.0, span.hi.0));
    }

    fn is_excluded(&self, span: Span) -> bool {
        self.excluded.contains(&(span.lo.0, span.hi.0))
    }

    fn declare(&mut self, ident: &Ident, kind: BindingKind) {
        if ident.sym.is_empty() || ident.ctxt.has_mark(self.unresolved) {
            return;
        }
        let id = ident.to_id();
        let top_level = self.depth == 0;
        match self.analysis.bindings.get_mut(&id) {
            Some(info) => {
                info.references += 1;
            }
            None => {
                self.analysis.bindings.insert(
                    id,
                    BindingInfo {
                        kind,
                        name: ident.sym.to_string(),
                        references: 0,
                        top_level,
                        exported: false,
                        mutated: false,
                        span: ident.span,
                    },
                );
            }
        }
    }

    fn mark_exported(&mut self, id: &Id) {
        if let Some(info) = self.analysis.bindings.get_mut(id) {
            info.exported = true;
        }
        self.analysis.pinned.insert(id.clone());
    }

    fn mark_mutated(&mut self, pat: &Pat) {
        let mut ids = Vec::new();
        collect_pat_idents(pat, &mut ids);
        for id in ids {
            if let Some(info) = self.analysis.bindings.get_mut(&id) {
                info.mutated = true;
            }
        }
    }

    fn mark_mutated_target(&mut self, target: &AssignTarget) {
        match target {
            AssignTarget::Simple(SimpleAssignTarget::Ident(ident)) => {
                if ident.ctxt != SyntaxContext::empty() {
                    let id = ident.id.to_id();
                    if let Some(info) = self.analysis.bindings.get_mut(&id) {
                        info.mutated = true;
                    }
                }
            }
            AssignTarget::Pat(pat) => {
                let mut ids = Vec::new();
                collect_assign_target_pat_idents(pat, &mut ids);
                for id in ids {
                    if let Some(info) = self.analysis.bindings.get_mut(&id) {
                        info.mutated = true;
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_directive_spans(module: &Module) -> Vec<Span> {
    let mut spans = Vec::new();
    for item in &module.body {
        match item {
            ModuleItem::Stmt(Stmt::Expr(ExprStmt { expr, .. })) => {
                if let Expr::Lit(Lit::Str(s)) = &**expr {
                    spans.push(s.span);
                }
            }
            _ => break,
        }
    }
    spans
}

impl Visit for Collector<'_> {
    fn visit_ident(&mut self, ident: &Ident) {
        if ident.sym.is_empty() {
            return;
        }
        if ident.ctxt.has_mark(self.unresolved) || ident.ctxt == SyntaxContext::empty() {
            self.analysis.globals.insert(ident.sym.clone());
            return;
        }
        self.declare(ident, BindingKind::Other);
    }

    fn visit_var_decl(&mut self, decl: &VarDecl) {
        let kind = match decl.kind {
            VarDeclKind::Let => BindingKind::Let,
            VarDeclKind::Const => BindingKind::Const,
            VarDeclKind::Var => BindingKind::Var,
        };
        for declarator in &decl.decls {
            if let Pat::Ident(binding) = &declarator.name {
                self.declare(&binding.id, kind);
            }
            declarator.name.visit_with(self);
            if let Some(init) = &declarator.init {
                init.visit_with(self);
            }
            if decl.kind != VarDeclKind::Var && !matches!(declarator.name, Pat::Ident(_)) {
                self.mark_mutated(&declarator.name);
            }
        }
    }

    fn visit_fn_decl(&mut self, decl: &FnDecl) {
        self.declare(&decl.ident, BindingKind::Function);
        decl.function.visit_with(self);
    }

    fn visit_class_decl(&mut self, decl: &ClassDecl) {
        self.declare(&decl.ident, BindingKind::Class);
        self.depth += 1;
        decl.class.visit_with(self);
        self.depth -= 1;
    }

    fn visit_class_expr(&mut self, expr: &ClassExpr) {
        if let Some(ident) = &expr.ident {
            self.declare(ident, BindingKind::Class);
        }
        self.depth += 1;
        expr.class.visit_with(self);
        self.depth -= 1;
    }

    fn visit_fn_expr(&mut self, expr: &FnExpr) {
        if let Some(ident) = &expr.ident {
            self.declare(ident, BindingKind::Function);
        }
        expr.function.visit_with(self);
    }

    fn visit_arrow_expr(&mut self, expr: &ArrowExpr) {
        self.analysis.functions.push(arrow_info(expr));
        self.depth += 1;
        expr.params.visit_with(self);
        match &*expr.body {
            ArrowFunctionBody::FunctionBody(body) => self.visit_function_body(body),
            ArrowFunctionBody::Expr(e) => e.visit_with(self),
        }
        self.depth -= 1;
    }

    fn visit_function(&mut self, function: &Function) {
        let mut info = FunctionInfo {
            name: None,
            span: function.span,
            params: function.params.len(),
            statements: 0,
            branches: 0,
            is_async: function.is_async,
            is_generator: function.is_generator,
            top_level: self.depth == 0,
            flattenable: false,
            is_arrow: false,
        };
        if let Some(body) = &function.body {
            info.statements = body.stmts.len();
            info.branches = count_branches(&body.stmts);
            info.flattenable = is_flattenable(&body.stmts);
        }
        self.analysis.functions.push(info);
        self.depth += 1;
        for param in &function.params {
            if let Pat::Ident(binding) = &param.pat {
                self.declare(&binding.id, BindingKind::Param);
            }
            param.pat.visit_with(self);
        }
        function.decorators.visit_with(self);
        if let Some(body) = &function.body {
            self.visit_function_body(body);
        }
        self.depth -= 1;
    }

    fn visit_export_decl(&mut self, decl: &ExportDecl) {
        decl.visit_children_with(self);
        let mut ids = Vec::new();
        match &decl.decl {
            Decl::Fn(function) => ids.push(function.ident.to_id()),
            Decl::Class(class) => ids.push(class.ident.to_id()),
            Decl::Var(var) => {
                for declarator in &var.decls {
                    collect_pat_idents(&declarator.name, &mut ids);
                }
            }
            _ => {}
        }
        for id in ids {
            self.mark_exported(&id);
        }
    }

    fn visit_private_name(&mut self, _private: &PrivateName) {}

    fn visit_str(&mut self, node: &Str) {
        if self.is_excluded(node.span) {
            return;
        }
        let value = node.value.to_atom_lossy().to_string();
        if value.is_empty() {
            return;
        }
        self.analysis.strings.push(StringSite {
            span: node.span,
            value,
            protected: false,
        });
    }

    fn visit_number(&mut self, node: &Number) {
        if self.is_excluded(node.span) {
            return;
        }
        self.analysis.numbers.push(NumberSite {
            span: node.span,
            value: node.value,
        });
    }

    fn visit_prop_name(&mut self, name: &PropName) {
        if let PropName::Ident(ident) = name {
            *self
                .analysis
                .properties
                .object_keys
                .entry(ident.sym.to_string())
                .or_insert(0) += 1;
        }
        if let PropName::Computed(computed) = name {
            if let Expr::Lit(Lit::Str(s)) = &*computed.expr {
                let key = s.value.to_atom_lossy().to_string();
                *self
                    .analysis
                    .properties
                    .computed_access
                    .entry(key)
                    .or_insert(0) += 1;
                self.exclude(s.span);
            } else {
                self.analysis.properties.unknown_access += 1;
                computed.expr.visit_with(self);
            }
        }
        if let PropName::Str(s) = name {
            self.exclude(s.span);
        }
    }

    fn visit_member_expr(&mut self, member: &MemberExpr) {
        member.obj.visit_with(self);
        match &member.prop {
            MemberProp::Ident(ident) => {
                *self
                    .analysis
                    .properties
                    .member_reads
                    .entry(ident.sym.to_string())
                    .or_insert(0) += 1;
            }
            MemberProp::Computed(computed) => {
                if let Expr::Lit(Lit::Str(s)) = &*computed.expr {
                    let key = s.value.to_atom_lossy().to_string();
                    *self
                        .analysis
                        .properties
                        .computed_access
                        .entry(key)
                        .or_insert(0) += 1;
                    self.exclude(s.span);
                } else {
                    self.analysis.properties.unknown_access += 1;
                    computed.expr.visit_with(self);
                }
            }
            MemberProp::PrivateName(_) => {}
        }
    }

    fn visit_assign_expr(&mut self, assign: &AssignExpr) {
        if !matches!(assign.op, AssignOp::Assign) {
            self.mark_mutated_target(&assign.left);
        }
        if let AssignTarget::Pat(pat) = &assign.left {
            let mut ids = Vec::new();
            collect_assign_target_pat_idents(pat, &mut ids);
            for id in ids {
                if let Some(info) = self.analysis.bindings.get_mut(&id) {
                    info.mutated = true;
                }
            }
        }
        assign.left.visit_with(self);
        assign.right.visit_with(self);
    }

    fn visit_update_expr(&mut self, update: &UpdateExpr) {
        match &*update.arg {
            Expr::Ident(ident) => {
                if ident.ctxt != SyntaxContext::empty() {
                    let id = ident.to_id();
                    if let Some(info) = self.analysis.bindings.get_mut(&id) {
                        info.mutated = true;
                    }
                }
            }
            Expr::Member(member) => match &member.prop {
                MemberProp::Ident(ident) => {
                    *self
                        .analysis
                        .properties
                        .member_writes
                        .entry(ident.sym.to_string())
                        .or_insert(0) += 1;
                }
                _ => self.analysis.properties.unknown_access += 1,
            },
            _ => {}
        }
        update.arg.visit_with(self);
    }

    fn visit_with_stmt(&mut self, with: &WithStmt) {
        self.analysis.has_with = true;
        with.obj.visit_with(self);
        with.body.visit_with(self);
    }

    fn visit_call_expr(&mut self, call: &CallExpr) {
        if let Callee::Expr(callee) = &call.callee {
            if let Expr::Ident(ident) = &**callee {
                let unresolved =
                    ident.ctxt.has_mark(self.unresolved) || ident.ctxt == SyntaxContext::empty();
                if ident.sym == *"eval" && unresolved {
                    self.analysis.has_eval = true;
                }
                if ident.sym == *"Function" && unresolved {
                    self.analysis.has_new_function = true;
                }
            }
        }
        call.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, expr: &NewExpr) {
        if let Expr::Ident(ident) = &*expr.callee {
            let unresolved =
                ident.ctxt.has_mark(self.unresolved) || ident.ctxt == SyntaxContext::empty();
            if ident.sym == *"Function" && unresolved {
                self.analysis.has_new_function = true;
            }
        }
        expr.visit_children_with(self);
    }

    fn visit_debugger_stmt(&mut self, _node: &DebuggerStmt) {
        self.analysis.has_debugger = true;
    }

    fn visit_import(&mut self, _node: &Import) {
        self.analysis.stats.dynamic_imports += 1;
    }

    fn visit_import_named_specifier(&mut self, spec: &ImportNamedSpecifier) {
        self.declare(&spec.local, BindingKind::Import);
        spec.local.visit_with(self);
    }

    fn visit_import_default_specifier(&mut self, spec: &ImportDefaultSpecifier) {
        self.declare(&spec.local, BindingKind::Import);
        spec.local.visit_with(self);
    }

    fn visit_import_star_as_specifier(&mut self, spec: &ImportStarAsSpecifier) {
        self.declare(&spec.local, BindingKind::Import);
        spec.local.visit_with(self);
    }

    fn visit_named_export(&mut self, named: &NamedExport) {
        self.analysis.stats.exports += named.specifiers.len();
        for spec in &named.specifiers {
            match spec {
                ExportSpecifier::Named(named_spec) => {
                    if let ModuleExportName::Ident(orig) = &named_spec.orig {
                        self.mark_exported(&orig.to_id());
                    }
                    named_spec.orig.visit_with(self);
                }
                ExportSpecifier::Namespace(namespace) => namespace.name.visit_with(self),
                ExportSpecifier::Default(default) => default.exported.visit_with(self),
            }
        }
        if let Some(src) = &named.src {
            src.visit_with(self);
        }
    }
}

pub fn collect_pat_idents(pat: &Pat, out: &mut Vec<Id>) {
    match pat {
        Pat::Ident(bi) => out.push(bi.id.to_id()),
        Pat::Array(arr) => {
            for element in arr.elems.iter().flatten() {
                collect_pat_idents(element, out);
            }
        }
        Pat::Object(obj) => {
            for prop in &obj.props {
                match prop {
                    ObjectPatProp::KeyValue(kv) => collect_pat_idents(&kv.value, out),
                    ObjectPatProp::Assign(assign) => out.push(assign.key.to_id()),
                    ObjectPatProp::Rest(rest) => collect_pat_idents(&rest.arg, out),
                }
            }
        }
        Pat::Assign(assign) => collect_pat_idents(&assign.left, out),
        Pat::Rest(rest) => collect_pat_idents(&rest.arg, out),
        Pat::Expr(_) | Pat::Invalid(_) => {}
    }
}

pub fn collect_assign_target_pat_idents(pat: &AssignTargetPat, out: &mut Vec<Id>) {
    match pat {
        AssignTargetPat::Array(array) => {
            for element in array.elems.iter().flatten() {
                collect_pat_idents(element, out);
            }
        }
        AssignTargetPat::Object(object) => {
            for prop in &object.props {
                match prop {
                    ObjectPatProp::KeyValue(kv) => collect_pat_idents(&kv.value, out),
                    ObjectPatProp::Assign(assign) => out.push(assign.key.to_id()),
                    ObjectPatProp::Rest(rest) => collect_pat_idents(&rest.arg, out),
                }
            }
        }
        _ => {}
    }
}

pub fn is_hoistable_lexical(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Decl(Decl::Class(_)) | Stmt::Decl(Decl::Fn(_)) => true,
        Stmt::Decl(Decl::Var(var)) => matches!(var.kind, VarDeclKind::Let | VarDeclKind::Const),
        _ => false,
    }
}

pub fn contains_unsupported_stmt(stmts: &[Stmt]) -> bool {
    stmts
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Labeled(_) | Stmt::With(_)))
}

pub fn is_flattenable(stmts: &[Stmt]) -> bool {
    stmts.len() >= 4
        && count_branches(stmts) >= 1
        && !contains_unsupported_stmt(stmts)
        && !stmts.iter().any(is_hoistable_lexical)
}

pub fn count_branches(stmts: &[Stmt]) -> usize {
    let mut count = 0;
    for stmt in stmts {
        count += match stmt {
            Stmt::If(_) => 1,
            Stmt::Switch(sw) => sw.cases.len(),
            Stmt::Try(t) => 1 + t.handler.is_some() as usize,
            Stmt::For(_) | Stmt::ForIn(_) | Stmt::ForOf(_) | Stmt::While(_) | Stmt::DoWhile(_) => 1,
            Stmt::Block(block) => count_branches(&block.stmts),
            _ => 0,
        };
    }
    count
}

fn arrow_info(expr: &ArrowExpr) -> FunctionInfo {
    let (statements, branches, flattenable) = match &*expr.body {
        ArrowFunctionBody::FunctionBody(body) => (
            body.stmts.len(),
            count_branches(&body.stmts),
            is_flattenable(&body.stmts),
        ),
        ArrowFunctionBody::Expr(_) => (1, 0, false),
    };
    FunctionInfo {
        name: None,
        span: expr.span,
        params: expr.params.len(),
        statements,
        branches,
        is_async: expr.is_async,
        is_generator: expr.is_generator,
        top_level: false,
        flattenable,
        is_arrow: true,
    }
}

pub fn is_dynamic_access(expr: &Expr) -> bool {
    match expr {
        Expr::Member(member) => match &member.prop {
            MemberProp::Computed(computed) => {
                !matches!(&*computed.expr, Expr::Lit(Lit::Str(_) | Lit::Num(_)))
            }
            _ => false,
        },
        Expr::OptChain(chain) => match &*chain.base {
            OptChainBase::Member(member) => match &member.prop {
                MemberProp::Computed(computed) => {
                    !matches!(&*computed.expr, Expr::Lit(Lit::Str(_) | Lit::Num(_)))
                }
                _ => false,
            },
            OptChainBase::Call(_) => false,
        },
        Expr::Call(call) => call.args.iter().any(|arg| is_dynamic_access(&arg.expr)),
        _ => false,
    }
}

pub fn is_string_concat(expr: &Expr) -> bool {
    match expr {
        Expr::Bin(bin) => {
            matches!(bin.op, BinaryOp::Add)
                && (matches!(&*bin.left, Expr::Lit(Lit::Str(_)))
                    || matches!(&*bin.right, Expr::Lit(Lit::Str(_)))
                    || is_string_concat(&bin.left)
                    || is_string_concat(&bin.right))
        }
        _ => false,
    }
}
