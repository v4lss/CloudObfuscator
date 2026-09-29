use cloudobfuscator_runtime::{NameFactory, Rng, StringTable};
use swc_atoms::Atom;
use swc_common::{Span, SyntaxContext};
use swc_ecma_ast::Ident;

#[derive(Debug, Clone)]
pub struct TransformConfig {
    pub obfuscate_strings: bool,
    pub obfuscate_numbers: bool,
    pub rename_identifiers: bool,
    pub mangle_properties: bool,
    pub flatten_control_flow: bool,
    pub aggressive_control_flow: bool,
    pub opaque_predicates: bool,
    pub rename_prefix: String,
    pub max_flattened_statements: usize,
}

impl Default for TransformConfig {
    fn default() -> TransformConfig {
        TransformConfig {
            obfuscate_strings: true,
            obfuscate_numbers: true,
            rename_identifiers: true,
            mangle_properties: false,
            flatten_control_flow: true,
            aggressive_control_flow: false,
            opaque_predicates: true,
            rename_prefix: String::new(),
            max_flattened_statements: 40,
        }
    }
}

impl TransformConfig {
    pub fn with_prefix(mut self, prefix: &str) -> TransformConfig {
        self.rename_prefix = prefix.to_string();
        self
    }

    pub fn strings(mut self, value: bool) -> TransformConfig {
        self.obfuscate_strings = value;
        self
    }

    pub fn numbers(mut self, value: bool) -> TransformConfig {
        self.obfuscate_numbers = value;
        self
    }

    pub fn rename(mut self, value: bool) -> TransformConfig {
        self.rename_identifiers = value;
        self
    }

    pub fn properties(mut self, value: bool) -> TransformConfig {
        self.mangle_properties = value;
        self
    }

    pub fn control_flow(mut self, value: bool) -> TransformConfig {
        self.flatten_control_flow = value;
        self
    }

    pub fn aggressive(mut self, value: bool) -> TransformConfig {
        self.aggressive_control_flow = value;
        self
    }

    pub fn opaque(mut self, value: bool) -> TransformConfig {
        self.opaque_predicates = value;
        self
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PassStats {
    pub renamed: usize,
    pub extracted_strings: usize,
    pub protected_strings: usize,
    pub obfuscated_numbers: usize,
    pub mangled_properties: usize,
    pub flattened_functions: usize,
    pub opaque_predicates: usize,
    pub skipped: Vec<String>,
}

impl PassStats {
    pub fn merge(&mut self, other: PassStats) {
        self.renamed += other.renamed;
        self.extracted_strings += other.extracted_strings;
        self.protected_strings += other.protected_strings;
        self.obfuscated_numbers += other.obfuscated_numbers;
        self.mangled_properties += other.mangled_properties;
        self.flattened_functions += other.flattened_functions;
        self.opaque_predicates += other.opaque_predicates;
        self.skipped.extend(other.skipped);
    }
}

pub struct PassContext<'a> {
    pub rng: &'a mut Rng,
    pub names: &'a mut NameFactory,
    pub strings: StringTable,
    pub renameable: Vec<swc_ecma_ast::Id>,
    pub property_map: std::collections::HashMap<String, String>,
    pub stats: PassStats,
    pub config: TransformConfig,
    top_level: SyntaxContext,
}

impl<'a> PassContext<'a> {
    pub fn new(
        rng: &'a mut Rng,
        names: &'a mut NameFactory,
        strings: StringTable,
        config: TransformConfig,
    ) -> PassContext<'a> {
        PassContext {
            rng,
            names,
            strings,
            renameable: Vec::new(),
            property_map: std::collections::HashMap::new(),
            stats: PassStats::default(),
            config,
            top_level: SyntaxContext::empty(),
        }
    }

    pub fn ident(&mut self, span: Span) -> Ident {
        let name = self.names.next(self.rng);
        Ident::new(Atom::from(name), span, self.top_level)
    }

    pub fn fresh_local(&mut self, span: Span) -> Ident {
        self.ident(span)
    }

    pub fn num(&mut self) -> f64 {
        self.rng.range(2, 1000) as f64
    }
}
