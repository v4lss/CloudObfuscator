use cloudobfuscator_core::{
    ControlFlowMode, IdentifierStyle, ObfuscationConfig, Obfuscator, Preset, StringEncoding,
};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn run_node(label: &str, js: &str) -> String {
    let mut child = match Command::new("node")
        .arg("--input-type=commonjs")
        .arg("-e")
        .arg(js)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            eprintln!("node not available, skipping {label}");
            return String::new();
        }
    };

    let mut stdout_pipe = child.stdout.take().expect("stdout pipe");
    let mut stderr_pipe = child.stderr.take().expect("stderr pipe");
    let stdout_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stdout_pipe.read_to_string(&mut buffer);
        buffer
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = stderr_pipe.read_to_string(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("{label} did not terminate within 30s");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("{label} wait failed: {error}"),
        }
    }

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    assert!(
        stderr.is_empty(),
        "{label} wrote to stderr:\n{stderr}\n--- output ---\n{js}"
    );
    stdout
}

fn config() -> ObfuscationConfig {
    ObfuscationConfig {
        seed: Some(0xBADC0FFEE0DDF00D),
        minify_output: true,
        ..ObfuscationConfig::default()
    }
}

const SAMPLE: &str = r#"
"use strict";
const LABEL = "hello world from the fixture";
const REPEAT = "repeated-payload-value";
const TAGS = ["alpha", "beta", "gamma"];

function buildTable(size) {
  const rows = [];
  for (let index = 0; index < size; index += 1) {
    rows.push({ id: index, name: TAGS[index % TAGS.length], label: LABEL + REPEAT });
  }
  return rows;
}

function summarize(rows) {
  let total = 0;
  for (const row of rows) {
    if (row.id % 2 === 0) {
      total += row.name.length;
    } else {
      total -= 1;
    }
  }
  return total;
}

class Registry {
  constructor(entries) {
    this.entries = entries;
    this.limit = 3;
  }
  add(entry) {
    this.entries.push(entry);
    return this;
  }
  describe() {
    return LABEL + ":" + this.entries.length + ":" + this.limit;
  }
}

const registry = new Registry(buildTable(6));
registry.add({ id: 99, name: "delta", label: REPEAT });

const template = `rows=${registry.entries.length}`;
const computed = { [LABEL]: 1, nested: { deep: REPEAT } };

globalThis.__RESULT__ = {
  label: LABEL,
  total: summarize(registry.entries),
  describe: registry.describe(),
  template: template,
  computed: computed[LABEL],
  deep: computed.nested.deep,
  spread: { ...computed.nested }.deep,
  json: JSON.stringify(registry.entries.map(function (row) { return row.id; })),
};
"#;

#[test]
fn obfuscated_output_preserves_behaviour() {
    let obfuscator = Obfuscator::new(config());
    let (output, report) = obfuscator
        .obfuscate_source(SAMPLE, "fixture.js")
        .expect("obfuscate");

    assert!(
        report.warnings.is_empty(),
        "warnings: {:?}",
        report.warnings
    );
    assert!(!output.is_empty());

    let stdout = run_node("balanced", &output);
    let expected = run_node("baseline", SAMPLE);
    assert_eq!(stdout, expected, "behaviour diverged");
}

#[test]
fn every_preset_and_style_stays_executable() {
    for preset in [Preset::Light, Preset::Balanced, Preset::Strong] {
        for style in [
            IdentifierStyle::Random,
            IdentifierStyle::Hex,
            IdentifierStyle::Alpha,
            IdentifierStyle::Mixed,
            IdentifierStyle::Unicode,
            IdentifierStyle::Short,
        ] {
            let mut cfg = config();
            preset.configure(&mut cfg);
            cfg.identifier_style = style;
            cfg.console_trap = false;
            cfg.debug_protection = false;
            cfg.integrity_check = false;
            cfg.self_defending = false;

            let obfuscator = Obfuscator::new(cfg);
            let (output, _) = obfuscator
                .obfuscate_source(SAMPLE, "fixture.js")
                .expect("obfuscate");
            let label = format!("{preset:?}-{style:?}");
            let stdout = run_node(&label, &output);
            assert_eq!(stdout, run_node("baseline", SAMPLE), "{label} diverged");
        }
    }
}

#[test]
fn every_string_encoding_round_trips() {
    for encoding in [
        StringEncoding::XorChain,
        StringEncoding::Base64Custom,
        StringEncoding::RotateArray,
        StringEncoding::ReverseShift,
        StringEncoding::SplitHalves,
        StringEncoding::Mixed,
    ] {
        let mut cfg = config();
        cfg.string_encoding = encoding;
        cfg.string_threshold = 0;
        cfg.string_group_count = 4;
        let obfuscator = Obfuscator::new(cfg);
        let (output, report) = obfuscator
            .obfuscate_source(SAMPLE, "fixture.js")
            .expect("obfuscate");
        assert!(
            report.passes.extracted_strings > 0,
            "{encoding:?} extracted nothing"
        );
        let label = format!("{encoding:?}");
        let stdout = run_node(&label, &output);
        assert_eq!(stdout, run_node("baseline", SAMPLE), "{label} diverged");
    }
}

#[test]
fn aggressive_control_flow_and_properties_stay_executable() {
    let mut cfg = config();
    cfg.control_flow = ControlFlowMode::Aggressive;
    cfg.mangle_properties = true;
    cfg.opaque_predicates = true;
    cfg.obfuscate_numbers = true;
    let obfuscator = Obfuscator::new(cfg);
    let (output, report) = obfuscator
        .obfuscate_source(SAMPLE, "fixture.js")
        .expect("obfuscate");
    assert!(report.passes.flattened_functions > 0, "no flattening");
    assert!(report.passes.opaque_predicates > 0, "no opaque predicates");
    let stdout = run_node("aggressive", &output);
    assert_eq!(stdout, run_node("baseline", SAMPLE), "aggressive diverged");
}

#[test]
fn protected_string_contexts_are_never_encoded() {
    let source = r#"
"use strict";
const tag = String.raw`template-literal-kept`;
const key = "object-key-value";
const holder = { "quoted-key-kept": 1, [key]: 2 };
const load = () => import("dynamic-import-kept.mjs");
const tagged = String.raw`tagged-kept`;
process.stdout.write(tag + "|" + holder["quoted-key-kept"] + "|" + typeof load + "|" + typeof tagged);
"#;
    let mut cfg = config();
    cfg.string_threshold = 0;
    cfg.string_group_count = 4;
    cfg.rename_identifiers = false;
    let obfuscator = Obfuscator::new(cfg);
    let (output, report) = obfuscator
        .obfuscate_source(source, "protected.js")
        .expect("obfuscate");
    assert!(report.passes.extracted_strings > 0, "nothing was encoded");
    for literal in [
        "use strict",
        "template-literal-kept",
        "quoted-key-kept",
        "dynamic-import-kept.mjs",
    ] {
        assert!(output.contains(literal), "{literal} was encoded:\n{output}");
    }
    assert!(
        output.starts_with("\"use strict\""),
        "directive prologue is no longer first:\n{output}"
    );
    let stdout = run_node("protected", &output);
    assert!(
        stdout.contains("template-literal-kept|1|function|string"),
        "{stdout}"
    );
}

#[test]
fn esm_imports_and_exports_survive() {
    let source = r#"
import { helper } from "./helper.mjs";
import def from "./helper.mjs";
export const VALUE = helper("compute-me-long-enough") + def;
export function run() {
  return VALUE + "suffix-value";
}
export default run;
"#;
    let obfuscator = Obfuscator::new(config());
    let (output, report) = obfuscator
        .obfuscate_source(source, "esm.js")
        .expect("obfuscate");
    assert!(report.is_esm, "module syntax was not detected");
    assert!(
        output.contains("./helper.mjs"),
        "import source was rewritten:\n{output}"
    );
    assert!(output.contains("export"), "exports were lost:\n{output}");

    let dir = std::env::temp_dir().join(format!("cloudobfuscator-esm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("helper.mjs"),
        "export const helper = (value) => value.toUpperCase();\nexport default \"!\";\n",
    )
    .expect("write helper");
    std::fs::write(
        dir.join("main.mjs"),
        format!(
            r#"
{output}
if (run() !== "COMPUTE-ME-LONG-ENOUGH!suffix-value") {{ throw new Error("bad value " + run()); }}
process.stdout.write("esm-ok");
"#
        ),
    )
    .expect("write main");

    let output_result = Command::new("node")
        .arg(dir.join("main.mjs"))
        .current_dir(&dir)
        .output()
        .expect("run node");
    let stdout = String::from_utf8_lossy(&output_result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output_result.stderr).to_string();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(stderr.is_empty(), "esm wrote to stderr:\n{stderr}");
    assert!(stdout.contains("esm-ok"), "{stdout}");
}

#[test]
fn eval_inputs_skip_string_obfuscation() {
    let source = r#"
const code = "1 + 1";
const value = eval(code);
globalThis.__RESULT__ = { value: value, marker: "kept-literal" };
process.stdout.write(value + ":" + globalThis.__RESULT__.marker);
"#;
    let obfuscator = Obfuscator::new(config());
    let (output, report) = obfuscator
        .obfuscate_source(source, "eval.js")
        .expect("obfuscate");
    assert!(report.analysis.has_eval || !report.warnings.is_empty());
    assert!(
        output.contains("kept-literal"),
        "literal was obfuscated despite eval:\n{output}"
    );
    let stdout = run_node("eval", &output);
    assert!(stdout.contains("2:kept-literal"), "{stdout}");
}

#[test]
fn output_is_deterministic_for_a_fixed_seed() {
    let obfuscator = Obfuscator::new(config());
    let first = obfuscator
        .obfuscate_source(SAMPLE, "fixture.js")
        .expect("obfuscate")
        .0;
    let second = obfuscator
        .obfuscate_source(SAMPLE, "fixture.js")
        .expect("obfuscate")
        .0;
    assert_eq!(first, second, "same seed produced different output");
}

#[test]
fn plain_strings_are_not_obfuscated_without_uses() {
    let source = "globalThis.__RESULT__ = { v: 'tiny' };";
    let obfuscator = Obfuscator::new(config());
    let (output, report) = obfuscator
        .obfuscate_source(source, "tiny.js")
        .expect("obfuscate");
    assert_eq!(report.passes.extracted_strings, 0);
    assert!(output.contains("tiny"), "{output}");
}
