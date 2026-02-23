use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const DEFAULT_BASE: &str = "sillytavern-1.15.0/public";
const DEFAULT_TARGET: &str = "sillytavern-1.16.0/public";
const DEFAULT_LOCAL: &str = "src";
const DEFAULT_ROUTE_DIR: &str = "src/tauri/main/routes";
const DEFAULT_COMMAND_REGISTRY: &str = "src-tauri/src/presentation/commands/registry.rs";
const DEFAULT_OUT: &str = "docs/upstream-sync-1.16-plan/reports";

const IGNORED_PREFIXES: &[&str] = &["scripts/extensions/third-party/JS-Slash-Runner/"];

const REPORT_FILE_CLASSIFICATION: &str = "01-file-classification.json";
const REPORT_INJECTION_CONFLICTS: &str = "02-injection-conflicts.json";
const REPORT_ENDPOINT_GAP: &str = "03-endpoint-gap.json";
const REPORT_COMMAND_GAP: &str = "04-command-gap.json";
const REPORT_SUMMARY: &str = "summary.md";

const INJECTION_KEYWORDS: &[&str] = &[
    "init.js",
    "tauri-main.js",
    "APP_INITIALIZED",
    "CHAT_LOADED",
    "chatLoaded",
];

#[derive(Debug, Clone)]
struct AnalyzeOptions {
    project_root: PathBuf,
    base_dir: PathBuf,
    target_dir: PathBuf,
    local_dir: PathBuf,
    route_dir: PathBuf,
    command_registry: PathBuf,
    out_dir: PathBuf,
}

impl AnalyzeOptions {
    fn from_args(args: &[String]) -> Result<Self> {
        let project_root = detect_project_root()?;
        let mut opts = Self {
            base_dir: project_root.join(DEFAULT_BASE),
            target_dir: project_root.join(DEFAULT_TARGET),
            local_dir: project_root.join(DEFAULT_LOCAL),
            route_dir: project_root.join(DEFAULT_ROUTE_DIR),
            command_registry: project_root.join(DEFAULT_COMMAND_REGISTRY),
            out_dir: project_root.join(DEFAULT_OUT),
            project_root,
        };

        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if let Some((key, value)) = arg.split_once('=') {
                opts.set_option(key, value)?;
                index += 1;
                continue;
            }

            match arg.as_str() {
                "--base" | "--target" | "--local" | "--route-dir" | "--command-registry"
                | "--out" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .ok_or_else(|| anyhow!("Missing value for option {}", arg))?;
                    opts.set_option(arg, value)?;
                    index += 1;
                }
                _ => bail!(
                    "Unknown option '{}'. Use `fastools upsync analyze --help` for usage.",
                    arg
                ),
            }
        }

        opts.validate()?;
        Ok(opts)
    }

    fn set_option(&mut self, key: &str, value: &str) -> Result<()> {
        let resolved = resolve_path(&self.project_root, value);
        match key {
            "--base" => self.base_dir = resolved,
            "--target" => self.target_dir = resolved,
            "--local" => self.local_dir = resolved,
            "--route-dir" => self.route_dir = resolved,
            "--command-registry" => self.command_registry = resolved,
            "--out" => self.out_dir = resolved,
            _ => bail!("Unknown option '{}'", key),
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        ensure_existing_dir(&self.base_dir, "--base")?;
        ensure_existing_dir(&self.target_dir, "--target")?;
        ensure_existing_dir(&self.local_dir, "--local")?;
        ensure_existing_dir(&self.route_dir, "--route-dir")?;
        ensure_existing_file(&self.command_registry, "--command-registry")?;
        Ok(())
    }

    fn to_view(&self) -> AnalyzeOptionsView {
        AnalyzeOptionsView {
            project_root: normalize_path_string(&self.project_root),
            base_dir: normalize_path_string(&self.base_dir),
            target_dir: normalize_path_string(&self.target_dir),
            local_dir: normalize_path_string(&self.local_dir),
            route_dir: normalize_path_string(&self.route_dir),
            command_registry: normalize_path_string(&self.command_registry),
            out_dir: normalize_path_string(&self.out_dir),
            ignored_prefixes: IGNORED_PREFIXES
                .iter()
                .map(|item| (*item).to_string())
                .collect(),
        }
    }
}

#[derive(Debug)]
struct FileClassificationData {
    upstream_changed: Vec<String>,
    local_changed: Vec<String>,
    both_changed: Vec<String>,
    upstream_only_changed: Vec<String>,
    local_only_changed: Vec<String>,
}

#[derive(Serialize)]
struct AnalyzeOptionsView {
    project_root: String,
    base_dir: String,
    target_dir: String,
    local_dir: String,
    route_dir: String,
    command_registry: String,
    out_dir: String,
    ignored_prefixes: Vec<String>,
}

#[derive(Serialize)]
struct FileClassificationSummary {
    upstream_changed: usize,
    local_changed: usize,
    both_changed: usize,
    upstream_only_changed: usize,
    local_only_changed: usize,
}

#[derive(Serialize)]
struct FileClassificationReport {
    generated_at: String,
    options: AnalyzeOptionsView,
    summary: FileClassificationSummary,
    upstream_changed: Vec<String>,
    local_changed: Vec<String>,
    both_changed: Vec<String>,
    upstream_only_changed: Vec<String>,
    local_only_changed: Vec<String>,
}

#[derive(Serialize)]
struct InjectionConflictSummary {
    both_changed_total: usize,
    injection_conflicts: usize,
    conflict_ratio: f64,
}

#[derive(Serialize)]
struct InjectionConflictItem {
    path: String,
    injection_conflict: bool,
    reasons: Vec<String>,
}

#[derive(Serialize)]
struct InjectionConflictReport {
    generated_at: String,
    summary: InjectionConflictSummary,
    conflicts: Vec<InjectionConflictItem>,
}

#[derive(Serialize)]
struct EndpointGapSummary {
    endpoints_in_base: usize,
    endpoints_in_target: usize,
    route_patterns: usize,
    new_in_target: usize,
    new_in_target_unhandled: usize,
    target_unhandled_total: usize,
}

#[derive(Serialize)]
struct EndpointGapReport {
    generated_at: String,
    summary: EndpointGapSummary,
    new_in_target: Vec<String>,
    new_in_target_unhandled: Vec<String>,
    target_unhandled: Vec<String>,
    route_patterns: Vec<String>,
}

#[derive(Serialize)]
struct CommandGapSummary {
    invoked_commands: usize,
    registered_commands: usize,
    invoke_but_not_registered: usize,
}

#[derive(Serialize)]
struct CommandGapReport {
    generated_at: String,
    summary: CommandGapSummary,
    invoked_commands: Vec<String>,
    registered_commands: Vec<String>,
    invoke_but_not_registered: Vec<String>,
}

pub fn run_upsync_analyze_cli(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    let opts = AnalyzeOptions::from_args(args)?;
    println!("Running upsync analysis...");
    println!(
        "Project root: {}",
        normalize_path_string(&opts.project_root)
    );

    let classification = classify_files(&opts)?;
    let classification_report = FileClassificationReport {
        generated_at: now_rfc3339(),
        options: opts.to_view(),
        summary: FileClassificationSummary {
            upstream_changed: classification.upstream_changed.len(),
            local_changed: classification.local_changed.len(),
            both_changed: classification.both_changed.len(),
            upstream_only_changed: classification.upstream_only_changed.len(),
            local_only_changed: classification.local_only_changed.len(),
        },
        upstream_changed: classification.upstream_changed.clone(),
        local_changed: classification.local_changed.clone(),
        both_changed: classification.both_changed.clone(),
        upstream_only_changed: classification.upstream_only_changed.clone(),
        local_only_changed: classification.local_only_changed.clone(),
    };

    let injection_report = analyze_injection_conflicts(&opts, &classification.both_changed)?;
    let endpoint_gap_report = analyze_endpoint_gaps(&opts)?;
    let command_gap_report = analyze_command_gaps(&opts)?;

    fs::create_dir_all(&opts.out_dir).with_context(|| {
        format!(
            "Failed to create output directory {}",
            normalize_path_string(&opts.out_dir)
        )
    })?;

    write_json_report(
        &opts.out_dir.join(REPORT_FILE_CLASSIFICATION),
        &classification_report,
    )?;
    write_json_report(
        &opts.out_dir.join(REPORT_INJECTION_CONFLICTS),
        &injection_report,
    )?;
    write_json_report(
        &opts.out_dir.join(REPORT_ENDPOINT_GAP),
        &endpoint_gap_report,
    )?;
    write_json_report(&opts.out_dir.join(REPORT_COMMAND_GAP), &command_gap_report)?;
    write_summary_markdown(
        &opts.out_dir.join(REPORT_SUMMARY),
        &classification_report,
        &injection_report,
        &endpoint_gap_report,
        &command_gap_report,
    )?;

    println!();
    println!("Upsync analysis completed.");
    println!(
        "Reports written to: {}",
        normalize_path_string(&opts.out_dir)
    );
    println!("  - {}", REPORT_FILE_CLASSIFICATION);
    println!("  - {}", REPORT_INJECTION_CONFLICTS);
    println!("  - {}", REPORT_ENDPOINT_GAP);
    println!("  - {}", REPORT_COMMAND_GAP);
    println!("  - {}", REPORT_SUMMARY);
    Ok(())
}

pub fn print_help() {
    println!("fastools upsync analyze");
    println!();
    println!("Usage:");
    println!("  fastools upsync analyze [options]");
    println!();
    println!("Options:");
    println!(
        "  --base <path>              Base upstream directory (default: {})",
        DEFAULT_BASE
    );
    println!(
        "  --target <path>            Target upstream directory (default: {})",
        DEFAULT_TARGET
    );
    println!(
        "  --local <path>             Local frontend directory (default: {})",
        DEFAULT_LOCAL
    );
    println!(
        "  --route-dir <path>         Tauri route directory (default: {})",
        DEFAULT_ROUTE_DIR
    );
    println!(
        "  --command-registry <path>  Rust command registry file (default: {})",
        DEFAULT_COMMAND_REGISTRY
    );
    println!(
        "  --out <path>               Report output directory (default: {})",
        DEFAULT_OUT
    );
    println!("  --help                     Show this help message");
}

fn classify_files(opts: &AnalyzeOptions) -> Result<FileClassificationData> {
    let upstream_changed_set = build_changed_set(&opts.base_dir, &opts.target_dir)?;
    let local_changed_set = build_changed_set(&opts.base_dir, &opts.local_dir)?;

    let both_changed_set: BTreeSet<String> = upstream_changed_set
        .intersection(&local_changed_set)
        .cloned()
        .collect();
    let upstream_only_set: BTreeSet<String> = upstream_changed_set
        .difference(&local_changed_set)
        .cloned()
        .collect();
    let local_only_set: BTreeSet<String> = local_changed_set
        .difference(&upstream_changed_set)
        .cloned()
        .collect();

    Ok(FileClassificationData {
        upstream_changed: upstream_changed_set.into_iter().collect(),
        local_changed: local_changed_set.into_iter().collect(),
        both_changed: both_changed_set.into_iter().collect(),
        upstream_only_changed: upstream_only_set.into_iter().collect(),
        local_only_changed: local_only_set.into_iter().collect(),
    })
}

fn build_changed_set(base_dir: &Path, compare_dir: &Path) -> Result<BTreeSet<String>> {
    let base_files = collect_relative_files(base_dir)?;
    let compare_files = collect_relative_files(compare_dir)?;
    let all_paths: BTreeSet<String> = base_files.union(&compare_files).cloned().collect();

    let mut changed = BTreeSet::new();
    for rel in all_paths {
        if is_ignored_path(&rel) {
            continue;
        }

        if files_differ(base_dir, compare_dir, &rel)? {
            changed.insert(rel);
        }
    }

    Ok(changed)
}

fn collect_relative_files(root: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    for entry in WalkDir::new(root) {
        let entry = entry
            .with_context(|| format!("Failed to walk directory {}", normalize_path_string(root)))?;
        if !entry.file_type().is_file() {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(root)
            .with_context(|| {
                format!(
                    "Failed to strip prefix {} from {}",
                    normalize_path_string(root),
                    normalize_path_string(entry.path())
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        files.insert(relative);
    }
    Ok(files)
}

fn files_differ(base_dir: &Path, compare_dir: &Path, rel: &str) -> Result<bool> {
    let base_path = base_dir.join(rel);
    let compare_path = compare_dir.join(rel);
    let base_exists = base_path.exists();
    let compare_exists = compare_path.exists();

    if base_exists != compare_exists {
        return Ok(true);
    }
    if !base_exists {
        return Ok(false);
    }

    let base_meta = fs::metadata(&base_path).with_context(|| {
        format!(
            "Failed to read metadata for {}",
            normalize_path_string(&base_path)
        )
    })?;
    let compare_meta = fs::metadata(&compare_path).with_context(|| {
        format!(
            "Failed to read metadata for {}",
            normalize_path_string(&compare_path)
        )
    })?;

    if base_meta.is_file() != compare_meta.is_file() {
        return Ok(true);
    }
    if !base_meta.is_file() {
        return Ok(true);
    }
    if base_meta.len() != compare_meta.len() {
        return Ok(true);
    }

    let base_bytes = fs::read(&base_path)
        .with_context(|| format!("Failed to read file {}", normalize_path_string(&base_path)))?;
    let compare_bytes = fs::read(&compare_path).with_context(|| {
        format!(
            "Failed to read file {}",
            normalize_path_string(&compare_path)
        )
    })?;

    Ok(base_bytes != compare_bytes)
}

fn analyze_injection_conflicts(
    opts: &AnalyzeOptions,
    both_changed: &[String],
) -> Result<InjectionConflictReport> {
    let mut conflicts = Vec::with_capacity(both_changed.len());
    for path in both_changed {
        conflicts.push(detect_injection_conflict(opts, path)?);
    }

    let conflict_count = conflicts
        .iter()
        .filter(|item| item.injection_conflict)
        .count();
    let ratio = if both_changed.is_empty() {
        0.0
    } else {
        conflict_count as f64 / both_changed.len() as f64
    };

    Ok(InjectionConflictReport {
        generated_at: now_rfc3339(),
        summary: InjectionConflictSummary {
            both_changed_total: both_changed.len(),
            injection_conflicts: conflict_count,
            conflict_ratio: ratio,
        },
        conflicts,
    })
}

fn detect_injection_conflict(opts: &AnalyzeOptions, path: &str) -> Result<InjectionConflictItem> {
    let mut reasons = BTreeSet::new();

    match path {
        "index.html" => {
            reasons.insert("entrypoint_bootstrap".to_string());
        }
        "script.js" => {
            reasons.insert("script_event_and_transport".to_string());
        }
        "lib.js" => {
            reasons.insert("library_facade".to_string());
        }
        "scripts/extensions.js" => {
            reasons.insert("extension_runtime_integration".to_string());
        }
        _ => {}
    }

    if path.starts_with("tauri/") {
        reasons.insert("tauri_injection_module".to_string());
    }
    if path.starts_with("scripts/extensions/runtime/") {
        reasons.insert("third_party_runtime_integration".to_string());
    }

    if path == "script.js" || path == "index.html" {
        let base_text = read_text_if_exists(&opts.base_dir.join(path))?;
        let target_text = read_text_if_exists(&opts.target_dir.join(path))?;
        let local_text = read_text_if_exists(&opts.local_dir.join(path))?;

        for keyword in INJECTION_KEYWORDS {
            let base_has = contains_keyword(base_text.as_deref(), keyword);
            let target_has = contains_keyword(target_text.as_deref(), keyword);
            let local_has = contains_keyword(local_text.as_deref(), keyword);

            if base_has != target_has || base_has != local_has || target_has != local_has {
                reasons.insert(format!("keyword_delta:{}", keyword));
            }
        }
    }

    let reason_list: Vec<String> = reasons.into_iter().collect();
    Ok(InjectionConflictItem {
        path: path.to_string(),
        injection_conflict: !reason_list.is_empty(),
        reasons: reason_list,
    })
}

fn analyze_endpoint_gaps(opts: &AnalyzeOptions) -> Result<EndpointGapReport> {
    let endpoints_base = extract_static_endpoints(&opts.base_dir)?;
    let endpoints_target = extract_static_endpoints(&opts.target_dir)?;
    let route_patterns = extract_route_patterns(&opts.route_dir)?;

    let new_in_target: Vec<String> = endpoints_target
        .difference(&endpoints_base)
        .cloned()
        .collect();

    let new_in_target_unhandled: Vec<String> = new_in_target
        .iter()
        .filter(|endpoint| !is_endpoint_handled(endpoint, &route_patterns))
        .cloned()
        .collect();

    let target_unhandled: Vec<String> = endpoints_target
        .iter()
        .filter(|endpoint| !is_endpoint_handled(endpoint, &route_patterns))
        .cloned()
        .collect();

    Ok(EndpointGapReport {
        generated_at: now_rfc3339(),
        summary: EndpointGapSummary {
            endpoints_in_base: endpoints_base.len(),
            endpoints_in_target: endpoints_target.len(),
            route_patterns: route_patterns.len(),
            new_in_target: new_in_target.len(),
            new_in_target_unhandled: new_in_target_unhandled.len(),
            target_unhandled_total: target_unhandled.len(),
        },
        new_in_target,
        new_in_target_unhandled,
        target_unhandled,
        route_patterns: route_patterns.into_iter().collect(),
    })
}

fn analyze_command_gaps(opts: &AnalyzeOptions) -> Result<CommandGapReport> {
    let invoked_commands = extract_safe_invoke_commands(&opts.route_dir, &opts.local_dir)?;
    let registered_commands = extract_registered_commands(&opts.command_registry)?;

    let missing: Vec<String> = invoked_commands
        .difference(&registered_commands)
        .cloned()
        .collect();

    Ok(CommandGapReport {
        generated_at: now_rfc3339(),
        summary: CommandGapSummary {
            invoked_commands: invoked_commands.len(),
            registered_commands: registered_commands.len(),
            invoke_but_not_registered: missing.len(),
        },
        invoked_commands: invoked_commands.into_iter().collect(),
        registered_commands: registered_commands.into_iter().collect(),
        invoke_but_not_registered: missing,
    })
}

fn extract_static_endpoints(root: &Path) -> Result<BTreeSet<String>> {
    let endpoint_regex = Regex::new(r#"['"](/(?:api|csrf-token|version)[^'"\s]*)['"]"#)
        .context("Failed to compile endpoint regex")?;
    let mut endpoints = BTreeSet::new();

    for entry in WalkDir::new(root) {
        let entry = entry
            .with_context(|| format!("Failed to walk directory {}", normalize_path_string(root)))?;
        if !entry.file_type().is_file() || !is_endpoint_source_file(entry.path()) {
            continue;
        }

        let content = read_text_lossy(entry.path())?;
        for captures in endpoint_regex.captures_iter(&content) {
            if let Some(raw) = captures.get(1) {
                let endpoint = normalize_endpoint(raw.as_str());
                if is_relevant_endpoint(&endpoint) {
                    endpoints.insert(endpoint);
                }
            }
        }
    }

    Ok(endpoints)
}

fn extract_route_patterns(route_dir: &Path) -> Result<BTreeSet<String>> {
    let route_regex = Regex::new(r#"router\.(?:get|post|all)\(\s*['"]([^'"]+)['"]"#)
        .context("Failed to compile route regex")?;
    let mut routes = BTreeSet::new();

    for entry in WalkDir::new(route_dir) {
        let entry = entry.with_context(|| {
            format!(
                "Failed to walk route dir {}",
                normalize_path_string(route_dir)
            )
        })?;
        if !entry.file_type().is_file() || !has_js_extension(entry.path()) {
            continue;
        }

        let content = read_text_lossy(entry.path())?;
        for captures in route_regex.captures_iter(&content) {
            if let Some(raw) = captures.get(1) {
                routes.insert(normalize_endpoint(raw.as_str()));
            }
        }
    }

    Ok(routes)
}

fn extract_safe_invoke_commands(route_dir: &Path, local_dir: &Path) -> Result<BTreeSet<String>> {
    let invoke_regex = Regex::new(r#"safeInvoke\(\s*['"]([^'"]+)['"]"#)
        .context("Failed to compile invoke regex")?;
    let mut commands = BTreeSet::new();

    let mut scan_roots = vec![route_dir.to_path_buf()];
    let context_path = local_dir.join("tauri/main/context.js");
    if context_path.exists() {
        scan_roots.push(context_path);
    }

    for root in scan_roots {
        if root.is_file() {
            let content = read_text_lossy(&root)?;
            for captures in invoke_regex.captures_iter(&content) {
                if let Some(name) = captures.get(1) {
                    commands.insert(name.as_str().to_string());
                }
            }
            continue;
        }

        for entry in WalkDir::new(&root) {
            let entry = entry.with_context(|| {
                format!(
                    "Failed to walk invoke source {}",
                    normalize_path_string(&root)
                )
            })?;
            if !entry.file_type().is_file() || !has_js_extension(entry.path()) {
                continue;
            }

            let content = read_text_lossy(entry.path())?;
            for captures in invoke_regex.captures_iter(&content) {
                if let Some(name) = captures.get(1) {
                    commands.insert(name.as_str().to_string());
                }
            }
        }
    }

    Ok(commands)
}

fn extract_registered_commands(command_registry: &Path) -> Result<BTreeSet<String>> {
    let command_regex = Regex::new(r#"super::[a-zA-Z0-9_]+::([a-zA-Z0-9_]+)"#)
        .context("Failed to compile command regex")?;
    let content = read_text_lossy(command_registry)?;
    let mut commands = BTreeSet::new();

    for captures in command_regex.captures_iter(&content) {
        if let Some(name) = captures.get(1) {
            commands.insert(name.as_str().to_string());
        }
    }

    Ok(commands)
}

fn is_endpoint_handled(endpoint: &str, routes: &BTreeSet<String>) -> bool {
    for pattern in routes {
        if pattern.ends_with('*') {
            let prefix = pattern.trim_end_matches('*');
            if endpoint.starts_with(prefix) {
                return true;
            }
        } else if pattern == endpoint {
            return true;
        }
    }
    false
}

fn write_json_report<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let payload = serde_json::to_string_pretty(value).context("Failed to serialize JSON report")?;
    fs::write(path, payload)
        .with_context(|| format!("Failed to write report {}", normalize_path_string(path)))?;
    Ok(())
}

fn write_summary_markdown(
    summary_path: &Path,
    classification: &FileClassificationReport,
    injection: &InjectionConflictReport,
    endpoints: &EndpointGapReport,
    commands: &CommandGapReport,
) -> Result<()> {
    let mut text = String::new();
    text.push_str("# fastools upsync analyze summary\n\n");
    text.push_str(&format!(
        "- Generated at: `{}`\n",
        classification.generated_at
    ));
    text.push_str(&format!(
        "- Base dir: `{}`\n",
        classification.options.base_dir
    ));
    text.push_str(&format!(
        "- Target dir: `{}`\n",
        classification.options.target_dir
    ));
    text.push_str(&format!(
        "- Local dir: `{}`\n",
        classification.options.local_dir
    ));
    text.push_str(&format!(
        "- Route dir: `{}`\n",
        classification.options.route_dir
    ));
    text.push_str(&format!(
        "- Command registry: `{}`\n",
        classification.options.command_registry
    ));
    text.push_str(&format!(
        "- Out dir: `{}`\n\n",
        classification.options.out_dir
    ));

    text.push_str("## File Classification\n\n");
    text.push_str(&format!(
        "- Upstream changed: **{}**\n",
        classification.summary.upstream_changed
    ));
    text.push_str(&format!(
        "- Local changed: **{}**\n",
        classification.summary.local_changed
    ));
    text.push_str(&format!(
        "- Both changed: **{}**\n",
        classification.summary.both_changed
    ));
    text.push_str(&format!(
        "- Upstream-only changed: **{}**\n",
        classification.summary.upstream_only_changed
    ));
    text.push_str(&format!(
        "- Local-only changed: **{}**\n\n",
        classification.summary.local_only_changed
    ));

    text.push_str("### Both Changed (top 40)\n\n");
    for path in classification.both_changed.iter().take(40) {
        text.push_str(&format!("- `{}`\n", path));
    }
    text.push('\n');

    text.push_str("## Injection Conflicts\n\n");
    text.push_str(&format!(
        "- Total both-changed files: **{}**\n",
        injection.summary.both_changed_total
    ));
    text.push_str(&format!(
        "- Injection conflicts: **{}**\n",
        injection.summary.injection_conflicts
    ));
    text.push_str(&format!(
        "- Conflict ratio: **{:.2}%**\n\n",
        injection.summary.conflict_ratio * 100.0
    ));

    text.push_str("### Injection Conflict Files (top 40)\n\n");
    for item in injection
        .conflicts
        .iter()
        .filter(|entry| entry.injection_conflict)
        .take(40)
    {
        let reason = if item.reasons.is_empty() {
            "none".to_string()
        } else {
            item.reasons.join(", ")
        };
        text.push_str(&format!("- `{}`: {}\n", item.path, reason));
    }
    text.push('\n');

    text.push_str("## Endpoint Gaps\n\n");
    text.push_str(&format!(
        "- Endpoints in base: **{}**\n",
        endpoints.summary.endpoints_in_base
    ));
    text.push_str(&format!(
        "- Endpoints in target: **{}**\n",
        endpoints.summary.endpoints_in_target
    ));
    text.push_str(&format!(
        "- Route patterns: **{}**\n",
        endpoints.summary.route_patterns
    ));
    text.push_str(&format!(
        "- New in target: **{}**\n",
        endpoints.summary.new_in_target
    ));
    text.push_str(&format!(
        "- New in target but unhandled: **{}**\n",
        endpoints.summary.new_in_target_unhandled
    ));
    text.push_str(&format!(
        "- Target unhandled total: **{}**\n\n",
        endpoints.summary.target_unhandled_total
    ));

    text.push_str("### New in Target but Unhandled\n\n");
    for endpoint in &endpoints.new_in_target_unhandled {
        text.push_str(&format!("- `{}`\n", endpoint));
    }
    text.push('\n');

    text.push_str("## Command Gaps\n\n");
    text.push_str(&format!(
        "- Invoked commands: **{}**\n",
        commands.summary.invoked_commands
    ));
    text.push_str(&format!(
        "- Registered commands: **{}**\n",
        commands.summary.registered_commands
    ));
    text.push_str(&format!(
        "- Invoked but not registered: **{}**\n\n",
        commands.summary.invoke_but_not_registered
    ));

    text.push_str("### Invoked but Not Registered\n\n");
    for command in &commands.invoke_but_not_registered {
        text.push_str(&format!("- `{}`\n", command));
    }

    fs::write(summary_path, text).with_context(|| {
        format!(
            "Failed to write summary {}",
            normalize_path_string(summary_path)
        )
    })?;
    Ok(())
}

fn read_text_if_exists(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_text_lossy(path)?))
}

fn read_text_lossy(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("Failed to read {}", normalize_path_string(path)))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn contains_keyword(text: Option<&str>, keyword: &str) -> bool {
    text.map(|content| content.contains(keyword))
        .unwrap_or(false)
}

fn is_endpoint_source_file(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => {
            let lower = ext.to_ascii_lowercase();
            lower == "js" || lower == "mjs" || lower == "html"
        }
        None => false,
    }
}

fn has_js_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let lower = ext.to_ascii_lowercase();
            lower == "js" || lower == "mjs"
        })
        .unwrap_or(false)
}

fn normalize_endpoint(raw: &str) -> String {
    raw.split('?').next().unwrap_or(raw).to_string()
}

fn is_relevant_endpoint(endpoint: &str) -> bool {
    endpoint.starts_with("/api/") || endpoint == "/csrf-token" || endpoint == "/version"
}

fn is_ignored_path(rel: &str) -> bool {
    let normalized = rel.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    IGNORED_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(&prefix.to_ascii_lowercase()))
}

fn resolve_path(project_root: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        project_root.join(path)
    }
}

fn ensure_existing_dir(path: &Path, option_name: &str) -> Result<()> {
    if !path.exists() {
        bail!(
            "Path for {} does not exist: {}",
            option_name,
            normalize_path_string(path)
        );
    }
    if !path.is_dir() {
        bail!(
            "Path for {} is not a directory: {}",
            option_name,
            normalize_path_string(path)
        );
    }
    Ok(())
}

fn ensure_existing_file(path: &Path, option_name: &str) -> Result<()> {
    if !path.exists() {
        bail!(
            "Path for {} does not exist: {}",
            option_name,
            normalize_path_string(path)
        );
    }
    if !path.is_file() {
        bail!(
            "Path for {} is not a file: {}",
            option_name,
            normalize_path_string(path)
        );
    }
    Ok(())
}

fn detect_project_root() -> Result<PathBuf> {
    let current = env::current_dir().context("Failed to read current directory")?;
    for candidate in current.ancestors() {
        let looks_like_root =
            candidate.join("package.json").exists() && candidate.join("src-tauri").exists();
        if looks_like_root {
            return Ok(candidate.to_path_buf());
        }
    }

    bail!(
        "Failed to locate project root from {}. Run inside the repository.",
        normalize_path_string(&current)
    );
}

fn normalize_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn now_rfc3339() -> String {
    Local::now().to_rfc3339()
}
