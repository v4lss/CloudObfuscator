mod constants;
mod context;
mod control_flow;
mod opaque;
mod properties;
mod rename;
mod strings;

pub use context::{PassContext, PassStats, TransformConfig};

use cloudobfuscator_analysis::{ProgramAnalysis, PropertyMode};
use cloudobfuscator_runtime::{NameFactory, Rng, StringTable};
use swc_ecma_ast::Module;

pub struct Pipeline<'a> {
    context: PassContext<'a>,
    analysis: ProgramAnalysis,
    renameable: Vec<swc_ecma_ast::Id>,
    config: TransformConfig,
    property_mode: PropertyMode,
}

impl<'a> Pipeline<'a> {
    pub fn new(
        rng: &'a mut Rng,
        names: &'a mut NameFactory,
        strings: StringTable,
        analysis: ProgramAnalysis,
        config: TransformConfig,
        property_mode: PropertyMode,
    ) -> Pipeline<'a> {
        let renameable = analysis.rename_candidates();
        let mut context = PassContext::new(rng, names, strings, config.clone());
        context.renameable = renameable.clone();
        Pipeline {
            context,
            analysis,
            renameable,
            config,
            property_mode,
        }
    }

    pub fn run(&mut self, module: &mut Module) -> PassStats {
        let property_mode = self.property_mode;
        let analysis = &self.analysis;
        let context = &mut self.context;

        opaque::run(context, module);
        control_flow::run(context, module);
        let plan = properties::run(context, analysis, module, property_mode);
        context.property_map = plan
            .map
            .iter()
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect();
        constants::run(context, module);
        if context.config.obfuscate_strings {
            strings::run(context, module);
        }
        if context.config.rename_identifiers {
            let renameable = std::mem::take(&mut self.renameable);
            rename::run(context, &renameable, module);
            self.renameable = renameable;
        }

        self.context.stats.clone()
    }

    pub fn analysis(&self) -> &ProgramAnalysis {
        &self.analysis
    }

    pub fn stats(&self) -> &PassStats {
        &self.context.stats
    }

    pub fn strings(&self) -> &StringTable {
        &self.context.strings
    }

    pub fn config(&self) -> &TransformConfig {
        &self.config
    }
}
