mod json;
mod lint;

use lint::{Ctx, Finding, Severity, Source};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "hooklint - lint Claude Code hooks before they silently fail

USAGE:
    hooklint [OPTIONS] [PATH...]

PATH is a settings.json / settings.local.json / hooks.json file or a project
directory (default: .). In a directory hooklint reads .claude/settings.json,
.claude/settings.local.json and hooks/hooks.json (plugins).

OPTIONS:
    --user        also lint ~/.claude/settings.json
    --json        machine-readable output
    --strict      exit 1 on warnings too
    --quiet       only show errors
    --no-fs       skip checks that read script files on disk
    --no-color    disable colors
    -h, --help    show this help
    -V, --version show version

EXIT CODES: 0 clean, 1 problems found, 2 usage or read error";

struct Opts {
    paths: Vec<PathBuf>,
    user: bool,
    json: bool,
    strict: bool,
    quiet: bool,
    check_fs: bool,
    color: bool,
}

struct Target {
    path: PathBuf,
    source: Source,
    root: PathBuf,
}

fn main() -> ExitCode {
    let mut o = Opts {
        paths: Vec::new(),
        user: false,
        json: false,
        strict: false,
        quiet: false,
        check_fs: true,
        color: std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("hooklint {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--user" => o.user = true,
            "--json" => o.json = true,
            "--strict" => o.strict = true,
            "--quiet" | "-q" => o.quiet = true,
            "--no-fs" => o.check_fs = false,
            "--no-color" => o.color = false,
            s if s.starts_with('-') => {
                eprintln!("hooklint: unknown option {s}\n\n{USAGE}");
                return ExitCode::from(2);
            }
            s => o.paths.push(PathBuf::from(s)),
        }
    }
    if o.paths.is_empty() {
        o.paths.push(PathBuf::from("."));
    }
    let mut targets = Vec::new();
    for p in &o.paths {
        if p.is_dir() {
            targets.extend(discover(p));
        } else if p.is_file() {
            targets.push(classify(p));
        } else {
            eprintln!("hooklint: {}: no such file or directory", p.display());
            return ExitCode::from(2);
        }
    }
    if o.user {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let p = Path::new(&home).join(".claude").join("settings.json");
            if p.is_file() {
                targets.push(Target {
                    path: p,
                    source: Source::Settings,
                    root: std::env::current_dir().unwrap_or_default(),
                });
            }
        }
    }

    let mut results: Vec<(String, Vec<Finding>)> = Vec::new();
    for t in &targets {
        let text = match std::fs::read_to_string(&t.path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("hooklint: {}: {e}", t.path.display());
                return ExitCode::from(2);
            }
        };
        let findings = match json::parse(&text) {
            Ok(doc) => lint::lint(
                &doc,
                &Ctx {
                    source: t.source,
                    root: t.root.clone(),
                    check_fs: o.check_fs,
                },
            ),
            Err(e) => vec![Finding {
                severity: Severity::Error,
                rule: "invalid-json",
                line: e.line,
                col: e.col,
                message: format!("{} (Claude Code skips the whole file)", e.msg),
            }],
        };
        let findings: Vec<Finding> = findings
            .into_iter()
            .filter(|f| !o.quiet || f.severity == Severity::Error)
            .collect();
        results.push((display(&t.path), findings));
    }

    let count = |s: Severity| {
        results
            .iter()
            .flat_map(|(_, f)| f)
            .filter(|f| f.severity == s)
            .count()
    };
    let (errors, warnings, infos) = (
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info),
    );

    if o.json {
        print_json(&results, errors, warnings, infos);
    } else {
        print_text(&results, &o, errors, warnings, infos);
    }
    if errors > 0 || (o.strict && warnings > 0) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn display(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

fn discover(dir: &Path) -> Vec<Target> {
    let mut out = Vec::new();
    for rel in [
        ".claude/settings.json",
        ".claude/settings.local.json",
        "hooks/hooks.json",
    ] {
        let p = dir.join(rel);
        if p.is_file() {
            out.push(classify(&p));
        }
    }
    out
}

fn classify(p: &Path) -> Target {
    let parent = p.parent().unwrap_or(Path::new(".")).to_path_buf();
    let parent = if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent
    };
    let parent_name = parent
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let up = |d: &Path| {
        d.parent()
            .filter(|x| !x.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let is_hooks_json = p.file_name().map(|n| n == "hooks.json").unwrap_or(false);
    if is_hooks_json && parent_name == "hooks" {
        Target {
            path: p.to_path_buf(),
            source: Source::Plugin,
            root: up(&parent),
        }
    } else if parent_name == ".claude" {
        Target {
            path: p.to_path_buf(),
            source: Source::Settings,
            root: up(&parent),
        }
    } else {
        Target {
            path: p.to_path_buf(),
            source: if is_hooks_json {
                Source::Plugin
            } else {
                Source::Settings
            },
            root: parent,
        }
    }
}

fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n == 1 { "" } else { "s" })
}

fn print_text(results: &[(String, Vec<Finding>)], o: &Opts, e: usize, w: usize, i: usize) {
    if results.is_empty() {
        println!("hooklint: no hook config found (looked for .claude/settings.json, .claude/settings.local.json, hooks/hooks.json)");
        return;
    }
    for (path, findings) in results {
        if findings.is_empty() {
            println!("{} {}", paint(o.color, "32", "ok"), path);
            continue;
        }
        println!("{}", paint(o.color, "1;4", path));
        for f in findings {
            let (code, label) = match f.severity {
                Severity::Error => ("31", "error"),
                Severity::Warning => ("33", "warning"),
                Severity::Info => ("36", "info"),
            };
            println!(
                "  {:>7}  {}  {}  {}",
                format!("{}:{}", f.line, f.col),
                paint(o.color, code, &format!("{label:<7}")),
                f.message,
                paint(o.color, "2", f.rule)
            );
        }
        println!();
    }
    let summary = format!(
        "{}, {}, {} in {}",
        plural(e, "error"),
        plural(w, "warning"),
        plural(i, "note"),
        plural(results.len(), "file")
    );
    let code = if e > 0 {
        "1;31"
    } else if w > 0 {
        "1;33"
    } else {
        "1;32"
    };
    println!("{}", paint(o.color, code, &summary));
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn print_json(results: &[(String, Vec<Finding>)], e: usize, w: usize, i: usize) {
    let mut items = Vec::new();
    for (path, findings) in results {
        for f in findings {
            items.push(format!(
                "    {{\"file\": {}, \"line\": {}, \"column\": {}, \"severity\": {}, \"rule\": {}, \"message\": {}}}",
                esc(path),
                f.line,
                f.col,
                esc(f.severity.label()),
                esc(f.rule),
                esc(&f.message)
            ));
        }
    }
    let files: Vec<String> = results.iter().map(|(p, _)| esc(p)).collect();
    println!(
        "{{\n  \"files\": [{}],\n  \"errors\": {e},\n  \"warnings\": {w},\n  \"notes\": {i},\n  \"findings\": [\n{}\n  ]\n}}",
        files.join(", "),
        items.join(",\n")
    );
}
