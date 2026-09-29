use anyhow::{anyhow, Result};
use cloudobfuscator_analysis::{PropertyMode, ProgramAnalysis};
use cloudobfuscator_parser::ParsedModule;
use cloudobfuscator_runtime::{
    derive_seed, GuardSet, NameStyle, Rng, RuntimePlan, RuntimeSummary,
};
use cloudobfuscator_transform::{PassStats, Pipeline, TransformConfig};
use serde::{Deserialize, Serialize};
use swc_ecma_ast::{Expr, Lit, ModuleItem, Stmt};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StringEncoding {
    XorChain,
    Base64Custom,
    RotateArray,
    ReverseShift,
    SplitHalves,
    Mixed,
}

impl Default for StringEncoding {
    fn default() -> Self {
        StringEncoding::Mixed
    }
}

impl StringEncoding {
    pub fn label(&self) -> &'static str {
        match self {
            StringEncoding::XorChain => "xor-chain",
            StringEncoding::Base64Custom => "base64-custom",
            StringEncoding::RotateArray => "rotate-array",
            StringEncoding::ReverseShift => "reverse-shift",
            StringEncoding::SplitHalves => "split-halves",
            StringEncoding::Mixed => "mixed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentifierStyle {
    Random,
    Hex,
    Alpha,
    Mixed,
    Unicode,
    Short,
}

impl Default for IdentifierStyle {
    fn default() -> Self {
        IdentifierStyle::Random
    }
}

impl IdentifierStyle {
    pub fn to_runtime(self) -> Option<NameStyle> {
        match self {
            IdentifierStyle::Random => None,
            IdentifierStyle::Hex => Some(NameStyle::Hex),
            IdentifierStyle::Alpha => Some(NameStyle::Alpha),
            IdentifierStyle::Mixed => Some(NameStyle::Mixed),
            IdentifierStyle::Unicode => Some(NameStyle::Unicode),
            IdentifierStyle::Short => Some(NameStyle::Short),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlFlowMode {
    Off,
    Safe,
    Aggressive,
}

impl Default for ControlFlowMode {
    fn default() -> Self {
        ControlFlowMode::Safe
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Browser,
    Node,
    WebWorker,
}

impl Default for Target {
    fn default() -> Self {
        Target::Browser
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObfuscationConfig {
    pub enabled: bool,
    pub seed: Option<u64>,
    pub target: Target,
    pub minify_output: bool,
    pub debug_protection: bool,
    pub self_defending: bool,
    pub console_trap: bool,
    pub integrity_check: bool,
    pub string_encoding: StringEncoding,
    pub string_threshold: usize,
    pub string_group_count: usize,
    pub max_string_table_entries: usize,
    pub rename_identifiers: bool,
    pub identifier_style: IdentifierStyle,
    pub mangle_properties: bool,
    pub property_mode: PropertyMode,
    pub control_flow: ControlFlowMode,
    pub opaque_predicates: bool,
    pub obfuscate_numbers: bool,
    pub string_array_threshold: usize,
    pub string_array_group_count: usize,
    pub domain_appenders: bool,
}

impl Default for ObfuscationConfig {
    fn default() -> Self {
        ObfuscationConfig {
            enabled: true,
            seed: None,
            target: Target::Browser,
            minify_output: true,
            debug_protection: false,
            self_defending: false,
            console_trap: false,
            integrity_check: false,
            string_encoding: StringEncoding::Mixed,
            string_threshold: 8,
            string_group_count: 3,
            max_string_table_entries: 30_000,
            rename_identifiers: true,
            identifier_style: IdentifierStyle::Random,
            mangle_properties: false,
            property_mode: PropertyMode::Safe,
            control_flow: ControlFlowMode::Safe,
            opaque_predicates: true,
            obfuscate_numbers: true,
            string_array_threshold: 0,
            string_array_group_count: 1,
            domain_appenders: false,
        }
    }
}

impl ObfuscationConfig {
    pub fn guards(&self) -> GuardSet {
        GuardSet {
            debug_protection: self.debug_protection,
            self_defending: self.self_defending,
            console_trap: self.console_trap,
            integrity_check: self.integrity_check,
        }
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        let _ = preset;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    None,
    Light,
    Balanced,
    Strong,
}

impl Preset {
    pub fn parse(value: &str) -> Result<Preset> {
        match value.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Ok(Preset::None),
            "light" | "low" => Ok(Preset::Light),
            "balanced" | "medium" | "default" => Ok(Preset::Balanced),
            "strong" | "max" | "aggressive" => Ok(Preset::Strong),
            other => Err(anyhow!(
                "unknown preset {other:?}; expected none, light, balanced or strong"
            )),
        }
    }

    pub fn configure(self, config: &mut ObfuscationConfig) {
        match self {
            Preset::None => {
                *config = ObfuscationConfig {
                    enabled: false,
                    ..ObfuscationConfig::default()
                };
            }
            Preset::Light => {
                *config = ObfuscationConfig {
                    string_group_count: 1,
                    control_flow: ControlFlowMode::Off,
                    opaque_predicates: false,
                    obfuscate_numbers: false,
                    ..ObfuscationConfig::default()
                };
            }
            Preset::Balanced => {}
            Preset::Strong => {
                *config = ObfuscationConfig {
                    debug_protection: true,
                    self_defending: true,
                    console_trap: true,
                    integrity_check: true,
                    mangle_properties: true,
                    property_mode: PropertyMode::Safe,
                    control_flow: ControlFlowMode::Aggressive,
                    string_group_count: 5,
                    ..ObfuscationConfig::default()
                };
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformReport {
    pub file: String,
    pub seed: u64,
    pub name_style: NameStyle,
    pub target: Target,
    pub is_esm: bool,
    pub analysis: AnalysisReport,
    pub passes: PassStats,
    pub runtime: RuntimeSummary,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub bindings: usize,
    pub renameable: usize,
    pub strings: usize,
    pub numbers: usize,
    pub functions: usize,
    pub globals: usize,
    pub has_eval: bool,
    pub has_with: bool,
    pub has_debugger: bool,
    pub has_new_function: bool,
    pub unknown_member_access: usize,
    pub source_bytes: usize,
}

impl From<&ProgramAnalysis> for AnalysisReport {
    fn from(analysis: &ProgramAnalysis) -> AnalysisReport {
        AnalysisReport {
            bindings: analysis.bindings.len(),
            renameable: analysis.rename_candidates().len(),
            strings: analysis.strings.len(),
            numbers: analysis.numbers.len(),
            functions: analysis.functions.len(),
            globals: analysis.globals.len(),
            has_eval: analysis.has_eval,
            has_with: analysis.has_with,
            has_debugger: analysis.has_debugger,
            has_new_function: analysis.has_new_function,
            unknown_member_access: analysis.properties.unknown_access,
            source_bytes: analysis.stats.bytes,
        }
    }
}

pub fn load_config_file(path: &Path) -> Result<ObfuscationConfig> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| anyhow!("cannot read {}: {error}", path.display()))?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut config: ObfuscationConfig = match extension.as_str() {
        "json" => serde_json::from_str(&text)
            .map_err(|error| anyhow!("invalid JSON in {}: {error}", path.display()))?,
        "yml" | "yaml" => serde_yaml::from_str(&text)
            .map_err(|error| anyhow!("invalid YAML in {}: {error}", path.display()))?,
        other => {
            return Err(anyhow!(
                "unsupported config extension {other:?}; use .json, .yml or .yaml"
            ))
        }
    };
    let _ = &mut config;
    Ok(config)
}

pub struct Obfuscator {
    config: ObfuscationConfig,
}

impl Obfuscator {
    pub fn new(config: ObfuscationConfig) -> Obfuscator {
        Obfuscator { config }
    }

    pub fn config(&self) -> &ObfuscationConfig {
        &self.config
    }

    pub fn obfuscate_source(
        &self,
        source: &str,
        filename: &str,
    ) -> Result<(String, TransformReport)> {
        swc_common::GLOBALS.set(&swc_common::Globals::new(), || {
            let parsed = cloudobfuscator_parser::parse_in_scope(source, filename)?;
            let unresolved = parsed.unresolved_mark;
            let analysis = ProgramAnalysis::collect(&parsed.module, unresolved, source.len());
            self.transform_parsed(parsed, analysis)
        })
    }

    pub fn transform_parsed(
        &self,
        mut parsed: ParsedModule,
        analysis: ProgramAnalysis,
    ) -> Result<(String, TransformReport)> {
        let mut warnings = Vec::new();
        for (flag, message) in [
            (analysis.has_eval, "input uses eval; string obfuscation was skipped"),
            (
                analysis.has_with,
                "input uses with; identifier renaming was skipped",
            ),
            (
                analysis.has_new_function,
                "input builds functions from strings; property mangling was skipped",
            ),
        ] {
            if flag {
                warnings.push(message.to_string());
            }
        }

        let seed = derive_seed(self.config.seed);
        let mut rng = Rng::new(seed);
        let mut names = cloudobfuscator_runtime::NameFactory::new(&mut rng);
        for global in &analysis.globals {
            names.reserve(&global.to_string());
        }

        let candidates = string_candidates(&analysis, &self.config);
        let table = build_table(
            &mut rng,
            &mut names,
            &candidates,
            self.config.string_encoding,
            self.config.string_group_count,
        );

        let runtime_guards = if analysis.has_eval || analysis.has_with {
            GuardSet::default()
        } else {
            self.config.guards()
        };

        let plan = RuntimePlan::from_table(
            seed,
            self.config.identifier_style.to_runtime(),
            &mut rng,
            &mut names,
            table,
            runtime_guards,
        );

        for reserved in plan.reserved_names() {
            names.reserve(reserved);
        }

        let transform_config = TransformConfig {
            obfuscate_strings: !analysis.has_eval,
            obfuscate_numbers: self.config.obfuscate_numbers,
            rename_identifiers: self.config.rename_identifiers && !analysis.has_with,
            mangle_properties: self.config.mangle_properties && !analysis.has_new_function,
            flatten_control_flow: self.config.control_flow != ControlFlowMode::Off,
            aggressive_control_flow: self.config.control_flow == ControlFlowMode::Aggressive,
            opaque_predicates: self.config.opaque_predicates,
            rename_prefix: names.prefix.clone(),
            max_flattened_statements: 40,
        };

        let mut pipeline = Pipeline::new(
            &mut rng,
            &mut names,
            plan.strings.clone(),
            analysis,
            transform_config,
            self.config.property_mode,
        );

        let runtime_items = flatten_runtime(&plan.source)?;
        let mut prologue = Vec::new();
        while let Some(ModuleItem::Stmt(Stmt::Expr(stmt))) = parsed.module.body.first() {
            if !matches!(&*stmt.expr, Expr::Lit(Lit::Str(_))) {
                break;
            }
            prologue.push(parsed.module.body.remove(0));
        }
        prologue.extend(runtime_items);
        parsed.module.body.splice(0..0, prologue);
        let stats = pipeline.run(&mut parsed.module);
        warnings.extend(stats.skipped.clone());

        let output = cloudobfuscator_parser::emit(
            &parsed.module,
            parsed.cm.clone(),
            &parsed.comments,
            self.config.minify_output,
        )?;

        let report = TransformReport {
            file: parsed.filename.clone(),
            seed,
            name_style: plan.name_style,
            target: self.config.target,
            is_esm: parsed.is_esm,
            analysis: AnalysisReport::from(pipeline.analysis()),
            passes: stats,
            runtime: plan.summary(),
            input_bytes: parsed.stats.bytes,
            output_bytes: output.len(),
            warnings,
        };
        Ok((output, report))
    }
}

fn flatten_runtime(prelude: &str) -> Result<Vec<ModuleItem>> {
    if prelude.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed = cloudobfuscator_parser::parse_in_scope(prelude, "__cloudobfuscator_runtime.js")?;
    Ok(parsed.module.body)
}

fn string_candidates(analysis: &ProgramAnalysis, config: &ObfuscationConfig) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for site in &analysis.strings {
        if site.value.len() < config.string_threshold {
            continue;
        }
        *counts.entry(site.value.as_str()).or_insert(0) += 1;
    }
    let mut ordered: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(value, count)| (value.to_string(), count))
        .collect();
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ordered.truncate(config.max_string_table_entries);
    ordered.into_iter().map(|(value, _)| value).collect()
}

fn build_table(
    rng: &mut Rng,
    names: &mut cloudobfuscator_runtime::NameFactory,
    values: &[String],
    encoding: StringEncoding,
    groups: usize,
) -> cloudobfuscator_runtime::StringTable {
    use cloudobfuscator_runtime::EncoderKind;
    let groups = groups.clamp(1, 16);
    let explicit: Option<Vec<EncoderKind>> = match encoding {
        StringEncoding::XorChain => Some(vec![EncoderKind::XorChain]),
        StringEncoding::Base64Custom => Some(vec![EncoderKind::Base64Custom]),
        StringEncoding::RotateArray => Some(vec![EncoderKind::RotateArray]),
        StringEncoding::ReverseShift => Some(vec![EncoderKind::ReverseShift]),
        StringEncoding::SplitHalves => Some(vec![EncoderKind::SplitHalves]),
        StringEncoding::Mixed => None,
    };
    match explicit {
        Some(kinds) => {
            cloudobfuscator_runtime::StringTable::build_with_kinds(rng, names, values, &kinds)
        }
        None => {
            let pool = EncoderKind::all();
            let kinds: Vec<EncoderKind> = (0..groups)
                .map(|index| pool[index % pool.len()])
                .collect();
            cloudobfuscator_runtime::StringTable::build_with_kinds(rng, names, values, &kinds)
        }
    }
}
