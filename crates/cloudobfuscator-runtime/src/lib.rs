mod guards;
mod names;
mod rng;
mod strings;

pub use guards::{all_kinds, from_char_codes, GuardKind, GuardSet};
pub use names::{NameFactory, NameStyle};
pub use rng::{derive_seed, Rng};
pub use strings::{escape_js_string, EncoderKind, StringTable};

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub seed: Option<u64>,
    pub string_group_count: usize,
    pub guards: GuardSet,
    pub name_style: Option<NameStyle>,
}

impl Default for RuntimeConfig {
    fn default() -> RuntimeConfig {
        RuntimeConfig {
            seed: None,
            string_group_count: 3,
            guards: GuardSet::default(),
            name_style: None,
        }
    }
}

impl RuntimeConfig {
    pub fn seed(mut self, seed: u64) -> RuntimeConfig {
        self.seed = Some(seed);
        self
    }

    pub fn with_groups(mut self, groups: usize) -> RuntimeConfig {
        self.string_group_count = groups;
        self
    }

    pub fn with_style(mut self, style: NameStyle) -> RuntimeConfig {
        self.name_style = Some(style);
        self
    }

    pub fn with_guards(mut self, guards: GuardSet) -> RuntimeConfig {
        self.guards = guards;
        self
    }
}

pub struct RuntimePlan {
    pub seed: u64,
    pub name_style: NameStyle,
    pub strings: StringTable,
    pub guard_kinds: Vec<GuardKind>,
    pub prelude: String,
    pub source: String,
    pub reserved: Vec<String>,
    pub requested_style: Option<NameStyle>,
}

fn apply_style(
    rng: &mut Rng,
    names: &mut NameFactory,
    style: Option<NameStyle>,
) -> Option<NameStyle> {
    let style = style?;
    names.style = style;
    names.prefix = match style {
        NameStyle::Hex => {
            let mut prefix = String::from("_0x");
            prefix.push_str(&rng.sample_string(names::HEX_CHARS, 2));
            prefix
        }
        NameStyle::Short => String::from("_"),
        _ => String::new(),
    };
    Some(style)
}

impl RuntimePlan {
    pub fn build(config: RuntimeConfig, values: &[String]) -> RuntimePlan {
        let seed = derive_seed(config.seed);
        let mut rng = Rng::new(seed);
        let mut names = NameFactory::new(&mut rng);
        let style = apply_style(&mut rng, &mut names, config.name_style);

        let mut deduped: Vec<String> = Vec::new();
        for value in values {
            if !deduped.contains(value) {
                deduped.push(value.clone());
            }
        }

        let table = StringTable::build(&mut rng, &mut names, &deduped, config.string_group_count);
        RuntimePlan::from_table(seed, style, &mut rng, &mut names, table, config.guards)
    }

    pub fn from_table(
        seed: u64,
        style: Option<NameStyle>,
        rng: &mut Rng,
        names: &mut NameFactory,
        table: StringTable,
        guard_set: GuardSet,
    ) -> RuntimePlan {
        let requested_style = style;
        let name_style = match requested_style {
            Some(style) => {
                apply_style(rng, names, Some(style));
                style
            }
            None => names.style(),
        };

        let guard_kinds = guards::all_kinds(guard_set);
        let decoders = table.decoder_names();
        let arities = table.decoder_arities();
        let mut guard_source = String::new();
        for (index, kind) in guard_kinds.iter().enumerate() {
            let slot = index % decoders.len().max(1);
            let target = decoders.get(slot).cloned().unwrap_or_default();
            let arity = arities.get(slot).copied().unwrap_or(0);
            let source = match kind {
                GuardKind::DebugProtection => guards::debug_protection(rng, names, &target),
                GuardKind::SelfDefending => guards::self_defending(rng, names, &target),
                GuardKind::ConsoleTrap => guards::console_trap(rng, names),
                GuardKind::IntegrityCheck => guards::integrity_check(rng, names, &target, arity),
            };
            guard_source.push_str(&source);
            guard_source.push('\n');
        }

        let mut source = table.source();
        source.push_str(&guard_source);

        let reserved = names.issued().to_vec();

        RuntimePlan {
            seed,
            name_style,
            strings: table,
            guard_kinds,
            prelude: guard_source,
            source,
            reserved,
            requested_style,
        }
    }

    pub fn reserved_names(&self) -> &[String] {
        &self.reserved
    }

    pub fn summary(&self) -> RuntimeSummary {
        RuntimeSummary {
            seed: self.seed,
            name_style: self.name_style,
            encoders: self
                .strings
                .kind_labels()
                .into_iter()
                .map(|label| label.to_string())
                .collect(),
            decoder_count: self.strings.decoder_count(),
            string_count: self.strings.len(),
            guards: self
                .guard_kinds
                .iter()
                .map(|g| g.label().to_string())
                .collect(),
            prelude_bytes: self.source.len(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeSummary {
    pub seed: u64,
    pub name_style: NameStyle,
    pub encoders: Vec<String>,
    pub decoder_count: usize,
    pub string_count: usize,
    pub guards: Vec<String>,
    pub prelude_bytes: usize,
}
