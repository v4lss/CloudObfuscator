use cloudobfuscator_runtime::{GuardSet, NameStyle, RuntimeConfig, RuntimePlan};

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let guards = match which.as_str() {
        "debug-protection" => GuardSet {
            debug_protection: true,
            ..Default::default()
        },
        "self-defending" => GuardSet {
            self_defending: true,
            ..Default::default()
        },
        "console-trap" => GuardSet {
            console_trap: true,
            ..Default::default()
        },
        "integrity-check" => GuardSet {
            integrity_check: true,
            ..Default::default()
        },
        _ => GuardSet {
            debug_protection: true,
            self_defending: true,
            console_trap: true,
            integrity_check: true,
        },
    };
    let plan = RuntimePlan::build(
        RuntimeConfig::default()
            .seed(0xABCD_0001)
            .with_groups(2)
            .with_style(NameStyle::Hex)
            .with_guards(guards),
        &["alpha".to_string(), "beta".to_string(), "constructor".to_string()],
    );
    println!("=== PRELUDE ===");
    println!("{}", plan.prelude);
    println!("=== SOURCE ===");
    println!("{}", plan.source);
    println!("=== RESERVED ===");
    for name in plan.reserved_names() {
        println!("{name}");
    }
    if let Some(path) = std::env::args().nth(2) {
        let mut js = String::from("\"use strict\";\n");
        js.push_str(&plan.source);
        js.push_str("\nprocess.stdout.write(\"ok\\n\");\nprocess.exit(0);\n");
        std::fs::write(&path, js).expect("write harness");
    }
}
