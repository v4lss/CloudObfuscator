use cloudobfuscator_runtime::{GuardSet, NameFactory, NameStyle, Rng, RuntimeConfig, RuntimePlan};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn run_js(label: &str, js: &str) {
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
            return;
        }
    };

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    panic!("{label} did not terminate within 20s:\n{js}");
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("{label} wait failed: {error}"),
        }
    }

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }

    assert!(
        stdout.contains("ok"),
        "{label} produced no ok marker.\nstdout: {stdout}\nstderr: {stderr}\n--- source ---\n{js}"
    );
}

fn build(guards: GuardSet, seed: u64) -> RuntimePlan {
    let values = vec![
        "alpha".to_string(),
        "beta".to_string(),
        "constructor".to_string(),
    ];
    let config = RuntimeConfig::default()
        .seed(seed)
        .with_groups(2)
        .with_style(NameStyle::Hex)
        .with_guards(guards);
    RuntimePlan::build(config, &values)
}

#[test]
fn plain_runtime_executes_and_decodes() {
    let plan = build(GuardSet::default(), 0x1234_5678);
    let mut js = String::new();
    js.push_str("\"use strict\";\n");
    js.push_str(&plan.source);
    js.push('\n');
    for value in ["alpha", "beta", "constructor"] {
        let accessor = plan
            .strings
            .accessor(value)
            .unwrap_or_else(|| panic!("missing accessor for {value}"));
        js.push_str(&format!("if({accessor}!=={value:?}){{throw new Error(\"bad\");}}\n"));
    }
    js.push_str("process.stdout.write(\"runtime-ok\\n\");\n");
    run_js("plain", &js);
}

#[test]
fn every_guard_executes_without_hanging() {
    for (label, guards) in [
        (
            "debug-protection",
            GuardSet {
                debug_protection: true,
                ..Default::default()
            },
        ),
        (
            "self-defending",
            GuardSet {
                self_defending: true,
                ..Default::default()
            },
        ),
        (
            "console-trap",
            GuardSet {
                console_trap: true,
                ..Default::default()
            },
        ),
        (
            "integrity-check",
            GuardSet {
                integrity_check: true,
                ..Default::default()
            },
        ),
        (
            "all",
            GuardSet {
                debug_protection: true,
                self_defending: true,
                console_trap: true,
                integrity_check: true,
            },
        ),
    ] {
        let plan = build(guards, 0xABCD_0001);
        let mut js = String::new();
        js.push_str("\"use strict\";\n");
        js.push_str(&plan.source);
        js.push('\n');
        js.push_str("process.stdout.write(\"guards-ok\\n\");process.exit(0);\n");
        run_js(label, &js);
    }
}

#[test]
fn debug_protection_uses_reachable_clock() {
    for seed in [1u64, 2, 3, 5, 8, 13, 21, 34] {
        let plan = build(
            GuardSet {
                debug_protection: true,
                ..Default::default()
            },
            seed,
        );
        let probe_line = plan
            .prelude
            .lines()
            .find(|line| line.contains("Date.now()-"))
            .unwrap_or_else(|| panic!("no timed probe for seed {seed}"));
        assert!(
            probe_line.contains("Date.now()"),
            "seed {seed} lost the Date reference: {probe_line}"
        );
        assert!(
            !probe_line.contains("undefined"),
            "seed {seed} emitted an unresolved identifier: {probe_line}"
        );
    }
}

#[test]
fn debug_protection_never_keeps_the_event_loop_alive() {
    for seed in 0..24u64 {
        let plan = build(
            GuardSet {
                debug_protection: true,
                ..Default::default()
            },
            0x5EED_0000 + seed,
        );
        let mut js = String::from("\"use strict\";\n");
        js.push_str(&plan.source);
        js.push_str("\nprocess.stdout.write(\"ok\\n\");\n");
        run_js(&format!("debug-protection-seed-{seed}"), &js);
    }
}

#[test]
fn reserved_names_cover_every_generated_identifier() {
    let plan = build(
        GuardSet {
            debug_protection: true,
            self_defending: true,
            console_trap: true,
            integrity_check: true,
        },
        0xFEED_FACE,
    );
    let reserved = plan.reserved_names();
    assert!(!reserved.is_empty());
    let mut seen = std::collections::HashSet::new();
    for name in reserved {
        assert!(seen.insert(name.clone()), "duplicate reserved {name}");
    }
    for name in ["Date", "setInterval", "setTimeout", "clearInterval", "clearTimeout", "console"] {
        assert!(
            !seen.contains(name),
            "host global {name} must not be reserved"
        );
    }
}

#[test]
fn every_name_style_generates_a_valid_runtime() {
    for style in NameStyle::all() {
        let mut rng = Rng::new(77);
        let mut names = NameFactory::new(&mut rng);
        names.style = style;
        assert_eq!(names.style(), style);
        let plan = RuntimePlan::build(
            RuntimeConfig::default()
                .seed(0x0BAD_C0DE)
                .with_groups(2)
                .with_style(style)
                .with_guards(GuardSet {
                    console_trap: true,
                    ..Default::default()
                }),
            &["x".to_string(), "yy".to_string()],
        );
        let mut js = String::new();
        js.push_str("\"use strict\";\n");
        js.push_str(&plan.source);
        js.push('\n');
        js.push_str("process.stdout.write(\"style-ok\\n\");\n");
        run_js("style", &js);
    }
}
