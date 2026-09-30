use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn binary() -> PathBuf {
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("cloudobfuscator.exe")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("cloudobfuscator-cli-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, contents).expect("write fixture");
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("run cloudobfuscator")
}

const SCRIPT: &str =
    "const message = \"hello from the cli integration fixture\";\nprocess.stdout.write(message);\n";

#[test]
fn single_file_writes_to_output_and_keeps_the_input_untouched() {
    let dir = scratch("single-file");
    let input = dir.join("app.js");
    let output = dir.join("out").join("app.js");
    write(&input, SCRIPT);

    let result = run(&[
        input.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--seed",
        "99",
        "--quiet",
    ]);

    assert!(result.status.success(), "{:?}", result);
    assert!(output.exists(), "output file was not created");
    assert_eq!(
        std::fs::read_to_string(&input).unwrap(),
        SCRIPT,
        "the input file must not be modified"
    );
    let obfuscated = std::fs::read_to_string(&output).unwrap();
    assert!(
        !obfuscated.contains("hello from the cli"),
        "the string literal was not extracted:\n{obfuscated}"
    );
    assert!(
        obfuscated.len() > SCRIPT.len(),
        "obfuscation should grow the file"
    );
}

#[test]
fn out_dir_keeps_the_input_layout() {
    let dir = scratch("out-dir");
    write(&dir.join("src").join("app.js"), SCRIPT);
    write(
        &dir.join("src").join("nested").join("mod.mjs"),
        "export const NAME = \"nested module value\";\n",
    );
    write(&dir.join("src").join("notes.md"), "ignored\n");
    write(
        &dir.join("src").join("types.d.ts"),
        "export type A = string;\n",
    );

    let result = run(&[
        dir.join("src").to_str().unwrap(),
        "--out-dir",
        dir.join("dist").to_str().unwrap(),
        "--quiet",
    ]);

    assert!(result.status.success(), "{:?}", result);
    assert!(dir.join("dist").join("app.js").exists());
    assert!(dir.join("dist").join("nested").join("mod.mjs").exists());
    assert!(!dir.join("dist").join("notes.md").exists());
    assert!(!dir.join("dist").join("types.d.ts").exists());
    assert!(
        dir.join("src").join("app.js").exists(),
        "the input tree must not be modified"
    );
}

#[test]
fn the_same_seed_produces_the_same_bytes() {
    let dir = scratch("determinism");
    let input = dir.join("app.js");
    write(&input, SCRIPT);
    let first = dir.join("first.js");
    let second = dir.join("second.js");

    for target in [&first, &second] {
        let result = run(&[
            input.to_str().unwrap(),
            "-o",
            target.to_str().unwrap(),
            "--seed",
            "12345",
            "--quiet",
        ]);
        assert!(result.status.success(), "{target:?}: {result:?}");
    }

    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap(),
        "a fixed seed must be reproducible"
    );
}

#[test]
fn different_seeds_diverge() {
    let dir = scratch("seed-divergence");
    let input = dir.join("app.js");
    write(&input, SCRIPT);
    let first = dir.join("first.js");
    let second = dir.join("second.js");

    for (seed, target) in [("1", &first), ("2", &second)] {
        let result = run(&[
            input.to_str().unwrap(),
            "-o",
            target.to_str().unwrap(),
            "--seed",
            seed,
            "--quiet",
        ]);
        assert!(result.status.success(), "{target:?}: {result:?}");
    }

    assert_ne!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap(),
        "different seeds should not collapse to the same output"
    );
}

#[test]
fn json_config_is_honoured_and_rejects_conflicts() {
    let dir = scratch("config");
    let input = dir.join("app.js");
    write(&input, SCRIPT);
    let config = dir.join("config.json");
    write(
        &config,
        "{\n  \"seed\": 4242,\n  \"minify_output\": false,\n  \"string_threshold\": 8\n}\n",
    );
    let output = dir.join("out.js");

    let result = run(&[
        input.to_str().unwrap(),
        "-c",
        config.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--quiet",
    ]);

    assert!(result.status.success(), "{result:?}");
    let obfuscated = std::fs::read_to_string(&output).unwrap();
    assert!(
        obfuscated.contains('\n'),
        "minify_output=false should keep line breaks"
    );

    let conflict = run(&[
        input.to_str().unwrap(),
        "-c",
        config.to_str().unwrap(),
        "-p",
        "strong",
    ]);
    assert!(
        !conflict.status.success(),
        "--config and --preset must conflict"
    );
}

#[test]
fn none_preset_disables_every_pass() {
    let dir = scratch("none-preset");
    let input = dir.join("app.js");
    write(&input, SCRIPT);
    let output = dir.join("out.js");

    let result = run(&[
        input.to_str().unwrap(),
        "-p",
        "none",
        "-o",
        output.to_str().unwrap(),
        "--quiet",
    ]);

    assert!(result.status.success(), "{result:?}");
    let obfuscated = std::fs::read_to_string(&output).unwrap();
    assert!(
        obfuscated.contains("hello from the cli"),
        "preset none must not extract strings:\n{obfuscated}"
    );
}

#[test]
fn a_report_is_written_for_every_processed_file() {
    let dir = scratch("report");
    write(&dir.join("src").join("app.js"), SCRIPT);
    write(
        &dir.join("src").join("nested").join("mod.mjs"),
        "export const NAME = \"nested module value\";\n",
    );
    let report = dir.join("report.json");

    let result = run(&[
        dir.join("src").to_str().unwrap(),
        "--out-dir",
        dir.join("dist").to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
        "--quiet",
    ]);

    assert!(result.status.success(), "{result:?}");
    let text = std::fs::read_to_string(&report).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("report is valid JSON");
    let entries = parsed.as_array().expect("report is a JSON array");
    assert_eq!(entries.len(), 2, "one entry per file: {text}");
    for entry in entries {
        assert!(entry["file"].is_string());
        assert!(entry["output_bytes"].as_u64().unwrap() > 0);
    }
}

#[test]
fn invalid_input_fails_with_a_message() {
    let dir = scratch("errors");
    let missing = dir.join("does-not-exist.js");

    let result = run(&[missing.to_str().unwrap()]);
    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("neither a file nor a directory"),
        "unexpected stderr: {stderr}"
    );

    let broken = dir.join("broken.js");
    write(&broken, "function ( { unclosed\n");
    let failure = run(&[
        broken.to_str().unwrap(),
        "-o",
        dir.join("x.js").to_str().unwrap(),
    ]);
    assert!(!failure.status.success(), "a parse error must fail");
}

#[test]
fn multiple_inputs_require_an_output_target() {
    let dir = scratch("multi-input");
    let first = dir.join("one.js");
    let second = dir.join("two.js");
    write(&first, SCRIPT);
    write(&second, SCRIPT);

    let result = run(&[first.to_str().unwrap(), second.to_str().unwrap()]);
    assert!(!result.status.success(), "ambiguous output must fail");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("--out-dir"), "unexpected stderr: {stderr}");

    let with_dir = run(&[
        first.to_str().unwrap(),
        second.to_str().unwrap(),
        "--out-dir",
        dir.join("dist").to_str().unwrap(),
        "--quiet",
    ]);
    assert!(with_dir.status.success(), "{with_dir:?}");
    assert!(dir.join("dist").join("one.js").exists());
    assert!(dir.join("dist").join("two.js").exists());
}

#[test]
fn a_byte_order_mark_in_the_config_is_tolerated() {
    let dir = scratch("bom-config");
    let input = dir.join("app.js");
    write(&input, SCRIPT);
    let config = dir.join("config.json");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"{\"seed\": 7, \"minify_output\": true}\n");
    std::fs::write(&config, bytes).expect("write config");
    let output = dir.join("out.js");

    let result = run(&[
        input.to_str().unwrap(),
        "-c",
        config.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--quiet",
    ]);

    assert!(result.status.success(), "{result:?}");
    assert!(output.exists());
}

fn serve(preset: Option<&str>, requests: &[String]) -> Vec<serde_json::Value> {
    let mut command = Command::new(binary());
    command.arg("--server");
    if let Some(preset) = preset {
        command.args(["--preset", preset]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cloudobfuscator --server");
    {
        let mut stdin = child.stdin.take().expect("server stdin");
        for request in requests {
            writeln!(stdin, "{request}").expect("write request");
        }
    }
    let output = child.wait_with_output().expect("wait for the server");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("decode reply"))
        .collect()
}

#[test]
fn the_server_answers_one_reply_per_request() {
    let replies = serve(
        None,
        &[
            r#"{"id":1,"file":"one.js","code":"const a = \"first server fixture string\";"}"#
                .to_string(),
            r#"{"id":2,"file":"two.js","code":"const b = \"second server fixture string\";"}"#
                .to_string(),
        ],
    );

    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["id"], 1);
    assert_eq!(replies[1]["id"], 2);
    for reply in &replies {
        assert_eq!(reply["ok"], true, "{reply}");
        assert!(!reply["code"].as_str().unwrap().is_empty());
        assert!(reply["report"]["file"].is_string());
    }
    assert!(!replies[0]["code"]
        .as_str()
        .unwrap()
        .contains("first server fixture"));
}

#[test]
fn request_config_overrides_the_preset_from_the_command_line() {
    let fixture = "const label = \"preset merge fixture string\";".to_string();

    let disabled = serve(
        Some("none"),
        &[format!(
            r#"{{"id":1,"file":"a.js","code":{}}}"#,
            serde_json::to_string(&fixture).unwrap()
        )],
    );
    assert_eq!(disabled[0]["ok"], true);
    assert!(
        disabled[0]["code"]
            .as_str()
            .unwrap()
            .contains("preset merge fixture"),
        "the none preset must reach the request"
    );

    let enabled = serve(
        Some("none"),
        &[format!(
            r#"{{"id":1,"file":"a.js","code":{},"config":{{"enabled":true}}}}"#,
            serde_json::to_string(&fixture).unwrap()
        )],
    );
    assert_eq!(enabled[0]["ok"], true);
    assert!(
        !enabled[0]["code"]
            .as_str()
            .unwrap()
            .contains("preset merge fixture"),
        "the per request config must win over the preset"
    );
}

#[test]
fn the_server_survives_bad_code_and_bad_config() {
    let replies = serve(
        None,
        &[
            r#"{"id":1,"file":"bad.js","code":"function ( { oops"}"#.to_string(),
            r#"{"id":2,"file":"bad-config.js","code":"const a = 1;","config":{"nope":true}}"#
                .to_string(),
            r#"{"id":3,"file":"good.js","code":"const c = \"third server fixture string\";"}"#
                .to_string(),
        ],
    );

    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["ok"], false);
    assert!(replies[0]["error"].as_str().unwrap().contains("parse"));
    assert_eq!(replies[1]["ok"], false);
    assert!(replies[1]["error"]
        .as_str()
        .unwrap()
        .contains("invalid config"));
    assert_eq!(
        replies[2]["ok"], true,
        "the server must keep serving after errors"
    );
    assert!(!replies[2]["code"]
        .as_str()
        .unwrap()
        .contains("third server fixture"));
}

#[test]
fn malformed_lines_do_not_kill_the_server() {
    let replies = serve(
        None,
        &[
            "not json at all".to_string(),
            "   ".to_string(),
            r#"{"id":9,"file":"ok.js","code":"const a = \"fourth server fixture string\";"}"#
                .to_string(),
        ],
    );

    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["ok"], false);
    assert!(replies[0]["error"]
        .as_str()
        .unwrap()
        .contains("malformed request"));
    assert_eq!(replies[1]["id"], 9);
    assert_eq!(replies[1]["ok"], true);
}
