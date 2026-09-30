use anyhow::{anyhow, bail, Result};
use clap::{Parser, ValueEnum};
use cloudobfuscator_core::{load_config_file, ObfuscationConfig, Obfuscator, Preset, TransformReport};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EXTENSIONS: [&str; 8] = ["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx"];

#[derive(Parser)]
#[command(
    name = "cloudobfuscator",
    version,
    about = "AST based JavaScript and TypeScript obfuscator"
)]
struct Cli {
    /// Files or directories to obfuscate
    #[arg(value_name = "INPUT", required_unless_present = "server")]
    inputs: Vec<PathBuf>,

    /// Read obfuscation requests as JSON lines on stdin and answer on stdout
    #[arg(long)]
    server: bool,

    /// Write the result to this file; only valid with a single input
    #[arg(short, long, value_name = "FILE", conflicts_with = "out_dir")]
    output: Option<PathBuf>,

    /// Write results into this directory, keeping the input layout
    #[arg(long, value_name = "DIR", conflicts_with = "output")]
    out_dir: Option<PathBuf>,

    /// Configuration file (.json, .yml or .yaml)
    #[arg(short, long, value_name = "FILE", conflicts_with = "preset")]
    config: Option<PathBuf>,

    /// Preset to start from
    #[arg(short, long, value_name = "NAME")]
    preset: Option<PresetName>,

    /// Seed used to make the output reproducible
    #[arg(long, value_name = "NUMBER")]
    seed: Option<u64>,

    /// Keep the output readable instead of minifying it
    #[arg(long)]
    no_minify: bool,

    /// Write a JSON report with one entry per processed file
    #[arg(long, value_name = "FILE")]
    report: Option<PathBuf>,

    /// Do not print per file summaries
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Copy, Clone, ValueEnum)]
enum PresetName {
    None,
    Light,
    Balanced,
    Strong,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = if cli.server {
        serve(&cli).map(|()| 0)
    } else {
        run(&cli)
    };
    match result {
        Ok(failures) if failures == 0 => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("cloudobfuscator: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<usize> {
    let config = build_config(cli)?;
    let files = collect_inputs(&cli.inputs)?;

    if cli.output.is_some() && files.len() > 1 {
        bail!("--output expects a single input but {} files were found", files.len());
    }
    if cli.output.is_none() && cli.out_dir.is_none() && files.len() > 1 {
        bail!("{} files were found; use --out-dir or pass a single input", files.len());
    }

    let obfuscator = Obfuscator::new(config);
    let mut reports = Vec::with_capacity(files.len());
    let mut failures = 0;

    for file in &files {
        let label = display_path(&file.source);
        let source = match std::fs::read_to_string(&file.source) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{label}: cannot read: {error}");
                failures += 1;
                continue;
            }
        };
        match obfuscator.obfuscate_source(&source, &label) {
            Ok((output, report)) => {
                match write_output(cli, &file.relative, &output) {
                    Ok(()) => {
                        if !cli.quiet {
                            print_summary(&report);
                        }
                        reports.push(report);
                    }
                    Err(error) => {
                        eprintln!("{label}: cannot write: {error:#}");
                        failures += 1;
                    }
                }
            }
            Err(error) => {
                eprintln!("{label}: {error:#}");
                failures += 1;
            }
        }
    }

    if let Some(path) = &cli.report {
        let json = serde_json::to_string_pretty(&reports)
            .map_err(|error| anyhow!("cannot serialize report: {error}"))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, json)?;
    }

    if !cli.quiet {
        eprintln!(
            "processed {} of {} files",
            reports.len(),
            files.len()
        );
    }
    Ok(failures)
}

fn build_config(cli: &Cli) -> Result<ObfuscationConfig> {
    let mut config = match &cli.config {
        Some(path) => load_config_file(path)?,
        None => {
            let mut config = ObfuscationConfig::default();
            match cli.preset {
                Some(PresetName::None) => Preset::None.configure(&mut config),
                Some(PresetName::Light) => Preset::Light.configure(&mut config),
                Some(PresetName::Balanced) => Preset::Balanced.configure(&mut config),
                Some(PresetName::Strong) => Preset::Strong.configure(&mut config),
                None => {}
            }
            config
        }
    };
    if let Some(seed) = cli.seed {
        config.seed = Some(seed);
    }
    if cli.no_minify {
        config.minify_output = false;
    }
    Ok(config)
}

struct Input {
    source: PathBuf,
    relative: PathBuf,
}

fn collect_inputs(inputs: &[PathBuf]) -> Result<Vec<Input>> {
    let mut files: Vec<Input> = Vec::new();
    for input in inputs {
        if input.is_file() {
            let relative = input
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| input.clone());
            files.push(Input {
                source: input.clone(),
                relative,
            });
            continue;
        }
        if !input.is_dir() {
            bail!("{} is neither a file nor a directory", input.display());
        }
        for entry in walkdir::WalkDir::new(input)
            .into_iter()
            .filter_entry(|entry| {
                let name = entry.file_name().to_string_lossy();
                name != "node_modules" && name != ".git"
            })
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() || !is_obfuscatable(entry.path()) {
                continue;
            }
            files.push(Input {
                relative: entry
                    .path()
                    .strip_prefix(input)
                    .unwrap_or(entry.path())
                    .to_path_buf(),
                source: entry.into_path(),
            });
        }
    }
    files.sort_by(|left, right| left.source.cmp(&right.source));
    files.dedup_by(|left, right| left.source == right.source);
    if files.is_empty() {
        bail!("no JavaScript or TypeScript files were found");
    }
    Ok(files)
}

fn is_obfuscatable(path: &Path) -> bool {
    let name = match path.file_name().and_then(|value| value.to_str()) {
        Some(name) => name,
        None => return false,
    };
    if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
        return false;
    }
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| EXTENSIONS.contains(&value.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn write_output(cli: &Cli, relative: &Path, output: &str) -> Result<()> {
    let destination = match (&cli.output, &cli.out_dir) {
        (Some(path), _) => path.clone(),
        (None, Some(dir)) => dir.join(relative),
        (None, None) => return write_stdout(output),
    };
    if let Some(parent) = destination.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&destination, output)?;
    Ok(())
}

fn write_stdout(output: &str) -> Result<()> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(output.as_bytes())?;
    if !output.ends_with('\n') {
        handle.write_all(b"\n")?;
    }
    handle.flush()?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct ServerRequest {
    id: u64,
    file: String,
    code: String,
    #[serde(default)]
    config: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(serde::Serialize)]
struct ServerResponse {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<TransformReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn serve(cli: &Cli) -> Result<()> {
    use std::io::{BufRead, Write};
    let fallback = build_config(cli)?;
    let base = match serde_json::to_value(&fallback)
        .map_err(|error| anyhow!("cannot encode fallback config: {error}"))?
    {
        serde_json::Value::Object(base) => base,
        other => anyhow::bail!("fallback config is not a JSON object: {other}"),
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<ServerRequest>(&line) {
            Ok(request) => {
                let config = match request.config {
                    Some(overrides) => {
                        let mut merged = base.clone();
                        merged.extend(overrides);
                        serde_json::from_value(serde_json::Value::Object(merged))
                    }
                    None => Ok(fallback.clone()),
                };
                match config {
                    Ok(config) => match Obfuscator::new(config)
                        .obfuscate_source(&request.code, &request.file)
                    {
                        Ok((code, report)) => ServerResponse {
                            id: request.id,
                            ok: true,
                            code: Some(code),
                            report: Some(report),
                            error: None,
                        },
                        Err(error) => ServerResponse {
                            id: request.id,
                            ok: false,
                            code: None,
                            report: None,
                            error: Some(format!("{error:#}")),
                        },
                    },
                    Err(error) => ServerResponse {
                        id: request.id,
                        ok: false,
                        code: None,
                        report: None,
                        error: Some(format!("invalid config: {error}")),
                    },
                }
            }
            Err(error) => ServerResponse {
                id: 0,
                ok: false,
                code: None,
                report: None,
                error: Some(format!("malformed request: {error}")),
            },
        };
        let encoded =
            serde_json::to_string(&response).map_err(|error| anyhow!("cannot encode reply: {error}"))?;
        writeln!(stdout, "{encoded}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn print_summary(report: &TransformReport) {
    let passes = &report.passes;
    eprintln!(
        "{} -> {} bytes, renamed {}, strings {}, numbers {}, properties {}, flattened {}",
        report.file,
        report.output_bytes,
        passes.renamed,
        passes.extracted_strings,
        passes.obfuscated_numbers,
        passes.mangled_properties,
        passes.flattened_functions,
    );
    for warning in &report.warnings {
        eprintln!("  warning: {warning}");
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
