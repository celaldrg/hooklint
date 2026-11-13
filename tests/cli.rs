use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hooklint"))
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hooklint-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join(".claude")).unwrap();
    d
}

fn run(dir: &PathBuf, settings: &str, extra: &[&str]) -> (i32, String) {
    fs::write(dir.join(".claude/settings.json"), settings).unwrap();
    let out = bin()
        .args(extra)
        .arg("--no-color")
        .arg(dir)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

#[test]
fn broken_example_reports_known_problems() {
    let out = bin()
        .args(["--no-color", "examples/broken"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1));
    for rule in [
        "matcher-case",
        "unknown-event",
        "mcp-matcher-exact",
        "header-env-not-allowed",
        "exit-1-does-not-block",
        "missing-script",
        "matcher-ignored",
        "unsupported-type",
        "invalid-if",
        "unknown-key",
        "dev-tty",
    ] {
        assert!(text.contains(rule), "missing {rule} in:\n{text}");
    }
    assert!(text.contains("did you mean \"UserPromptSubmit\""));
}

#[test]
fn clean_example_passes() {
    let out = bin()
        .args(["--no-color", "examples/clean"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn invalid_json_is_an_error_with_position() {
    let d = tmp("badjson");
    let (code, out) = run(&d, "{\n  \"hooks\": {},\n}", &[]);
    assert_eq!(code, 1);
    assert!(out.contains("3:1"), "{out}");
    assert!(out.contains("invalid-json"));
}

#[test]
fn if_on_non_tool_event_never_runs() {
    let d = tmp("ifstop");
    let (code, out) = run(
        &d,
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"true","if":"Bash(git *)"}]}]}}"#,
        &[],
    );
    assert_eq!(code, 1);
    assert!(out.contains("if-never-runs"), "{out}");
}

#[test]
fn warnings_fail_only_in_strict_mode() {
    let d = tmp("strict");
    let s = r#"{"hooks":{"PostToolUse":[{"matcher":"Edit.*","hooks":[{"type":"command","command":"true"}]}]}}"#;
    let (code, out) = run(&d, s, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("matcher-unanchored"));
    let (code, _) = run(&d, s, &["--strict"]);
    assert_eq!(code, 1);
}

#[test]
fn hardcoded_secret_in_http_header() {
    let d = tmp("secret");
    let (code, out) = run(
        &d,
        r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"http","url":"https://x.example","headers":{"Authorization":"Bearer abcdefghijklmnopqrstuvwxyz123456"}}]}]}}"#,
        &[],
    );
    assert_eq!(code, 1);
    assert!(out.contains("hardcoded-secret"), "{out}");
}

#[test]
fn exec_form_with_spaces_in_command() {
    let d = tmp("exec");
    let (code, out) = run(
        &d,
        r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"node script.js","args":["--fix"]}]}]}}"#,
        &[],
    );
    assert_eq!(code, 1);
    assert!(out.contains("exec-form-spaces"), "{out}");
}

#[test]
fn non_executable_script_detected() {
    let d = tmp("chmod");
    fs::create_dir_all(d.join(".claude/hooks")).unwrap();
    fs::write(d.join(".claude/hooks/x.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            d.join(".claude/hooks/x.sh"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let (code, out) = run(
            &d,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"\"$CLAUDE_PROJECT_DIR\"/.claude/hooks/x.sh"}]}]}}"#,
            &[],
        );
        assert_eq!(code, 1);
        assert!(out.contains("script-not-executable"), "{out}");
        let (code, _) = run(
            &d,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"bash \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/x.sh"}]}]}}"#,
            &[],
        );
        assert_eq!(code, 0);
    }
}

#[test]
fn json_output_is_parseable_shape() {
    let out = bin().args(["--json", "examples/broken"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.trim_start().starts_with('{'));
    assert!(text.contains("\"rule\": \"unknown-event\""));
    assert!(text.contains("\"errors\": "));
}

#[test]
fn plugin_hooks_json_uses_plugin_root() {
    let d = std::env::temp_dir().join(format!("hooklint-test-{}-plugin", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("hooks")).unwrap();
    fs::create_dir_all(d.join("scripts")).unwrap();
    fs::write(d.join("scripts/fmt.js"), "console.log(1)").unwrap();
    fs::write(
        d.join("hooks/hooks.json"),
        r#"{"description":"fmt","hooks":{"PostToolUse":[{"matcher":"Write|Edit","hooks":[{"type":"command","command":"node","args":["${CLAUDE_PLUGIN_ROOT}/scripts/fmt.js"]},{"type":"command","command":"node","args":["${CLAUDE_PLUGIN_ROOT}/scripts/missing.js"]}]}]}}"#,
    )
    .unwrap();
    let out = bin().args(["--no-color"]).arg(&d).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.matches("missing-script").count(), 1, "{text}");
    assert!(text.contains("missing.js"));
}
