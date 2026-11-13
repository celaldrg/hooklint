//! Rules for Claude Code hook configuration
//! (settings.json, settings.local.json and plugin hooks/hooks.json).

use crate::json::{Kind, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub rule: &'static str,
    pub line: usize,
    pub col: usize,
    pub message: String,
}

/// Where the file lives decides how script paths resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `.claude/settings.json`, `.claude/settings.local.json`, `~/.claude/settings.json`
    Settings,
    /// a plugin's `hooks/hooks.json`
    Plugin,
}

pub struct Ctx {
    pub source: Source,
    /// `${CLAUDE_PROJECT_DIR}` (settings) or `${CLAUDE_PLUGIN_ROOT}` (plugin)
    pub root: PathBuf,
    pub check_fs: bool,
}

pub const EVENTS: &[&str] = &[
    "SessionStart",
    "Setup",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "Notification",
    "MessageDisplay",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "TeammateIdle",
    "InstructionsLoaded",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeCreate",
    "WorktreeRemove",
    "PreCompact",
    "PostCompact",
    "PreModelSwitch",
    "PostModelSwitch",
    "Elicitation",
    "ElicitationResult",
    "SessionEnd",
];

const TOOL_EVENTS: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionDenied",
];

const NO_MATCHER: &[&str] = &[
    "UserPromptSubmit",
    "PostToolBatch",
    "Stop",
    "TeammateIdle",
    "TaskCreated",
    "TaskCompleted",
    "WorktreeCreate",
    "WorktreeRemove",
    "MessageDisplay",
    "CwdChanged",
];

const KNOWN_TOOLS: &[&str] = &[
    "Agent",
    "AskUserQuestion",
    "Bash",
    "BashOutput",
    "Edit",
    "ExitPlanMode",
    "Glob",
    "Grep",
    "KillShell",
    "LS",
    "Monitor",
    "MultiEdit",
    "NotebookEdit",
    "NotebookRead",
    "PowerShell",
    "Read",
    "Skill",
    "SlashCommand",
    "Task",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
    "Write",
];

fn enumerated_matchers(event: &str) -> Option<&'static [&'static str]> {
    Some(match event {
        "SessionStart" => &["startup", "resume", "clear", "compact", "fork"],
        "Setup" => &["init", "maintenance"],
        "SessionEnd" => &["clear", "resume", "logout", "prompt_input_exit", "other"],
        "PreCompact" | "PostCompact" => &["manual", "auto"],
        "ConfigChange" => &[
            "user_settings",
            "project_settings",
            "local_settings",
            "policy_settings",
            "skills",
        ],
        "DirectoryAdded" => &["slash_command", "register_repo_root"],
        "InstructionsLoaded" => &[
            "session_start",
            "nested_traversal",
            "path_glob_match",
            "include",
            "compact",
        ],
        "StopFailure" => &[
            "rate_limit",
            "overloaded",
            "authentication_failed",
            "oauth_org_not_allowed",
            "account_on_hold",
            "billing_error",
            "invalid_request",
            "model_not_found",
            "server_error",
            "max_output_tokens",
            "cloud_credential_error",
            "unknown",
        ],
        "Notification" => &[
            "permission_prompt",
            "idle_prompt",
            "auth_success",
            "auth_storage_failure",
            "elicitation_dialog",
            "elicitation_url_dialog",
            "elicitation_complete",
            "elicitation_response",
            "agent_needs_input",
            "agent_completed",
            "quota_auto_resume_fired",
            "quota_auto_resume_stale",
            "quota_auto_resume_disabled",
            "plugin_notification",
        ],
        _ => return None,
    })
}

const HANDLER_TYPES: &[&str] = &["command", "http", "mcp_tool", "prompt", "agent"];
const COMMON_KEYS: &[&str] = &["type", "if", "timeout", "statusMessage", "once"];

fn type_keys(t: &str) -> &'static [&'static str] {
    match t {
        "command" => &[
            "command",
            "args",
            "async",
            "asyncRewake",
            "shell",
            "onFailure",
        ],
        "http" => &["url", "headers", "allowedEnvVars", "onFailure"],
        "mcp_tool" => &["server", "tool", "input"],
        "prompt" | "agent" => &["prompt", "model"],
        _ => &[],
    }
}

pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn suggest<'a>(word: &str, options: &[&'a str]) -> Option<&'a str> {
    let limit = (word.chars().count() / 3).clamp(1, 3);
    options
        .iter()
        .map(|o| (edit_distance(word, o), *o))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, o)| o)
}

fn did_you_mean(word: &str, options: &[&str]) -> String {
    match suggest(word, options) {
        Some(s) => format!(" (did you mean \"{s}\"?)"),
        None => String::new(),
    }
}

struct Out {
    findings: Vec<Finding>,
}

impl Out {
    fn push(&mut self, sev: Severity, rule: &'static str, at: &Value, msg: String) {
        self.findings.push(Finding {
            severity: sev,
            rule,
            line: at.line,
            col: at.col,
            message: msg,
        });
    }
}

pub fn lint(doc: &Value, ctx: &Ctx) -> Vec<Finding> {
    let mut out = Out {
        findings: Vec::new(),
    };
    let Kind::Obj(_) = &doc.kind else {
        out.push(
            Severity::Error,
            "invalid-structure",
            doc,
            format!("top level must be an object, found {}", doc.type_name()),
        );
        return out.findings;
    };
    if let Some(v) = doc.get("disableAllHooks") {
        if v.kind == Kind::Bool(true) {
            out.push(
                Severity::Warning,
                "hooks-disabled",
                v,
                "\"disableAllHooks\": true turns off every hook in this file and lower-precedence files"
                    .into(),
            );
        }
    }
    let Some(hooks) = doc.get("hooks") else {
        return out.findings;
    };
    let Kind::Obj(events) = &hooks.kind else {
        out.push(
            Severity::Error,
            "invalid-structure",
            hooks,
            format!(
                "\"hooks\" must be an object keyed by event name, found {}",
                hooks.type_name()
            ),
        );
        return out.findings;
    };
    let mut seen: Vec<(String, String)> = Vec::new();
    for (event, groups) in events {
        if !EVENTS.contains(&event.as_str()) {
            out.push(
                Severity::Error,
                "unknown-event",
                groups,
                format!(
                    "\"{event}\" is not a hook event, so these hooks never run{}",
                    did_you_mean(event, EVENTS)
                ),
            );
            continue;
        }
        let Kind::Arr(groups_arr) = &groups.kind else {
            out.push(
                Severity::Error,
                "invalid-structure",
                groups,
                format!(
                    "\"{event}\" must be an array of matcher groups, found {}",
                    groups.type_name()
                ),
            );
            continue;
        };
        if groups_arr.is_empty() {
            out.push(
                Severity::Info,
                "empty-event",
                groups,
                format!("\"{event}\" has no matcher groups"),
            );
        }
        for group in groups_arr {
            lint_group(event, group, ctx, &mut out, &mut seen);
        }
    }
    out.findings.sort_by_key(|f| (f.line, f.col));
    out.findings
}

fn lint_group(
    event: &str,
    group: &Value,
    ctx: &Ctx,
    out: &mut Out,
    seen: &mut Vec<(String, String)>,
) {
    let Kind::Obj(fields) = &group.kind else {
        out.push(
            Severity::Error,
            "invalid-structure",
            group,
            format!(
                "matcher group must be an object, found {}",
                group.type_name()
            ),
        );
        return;
    };
    for (k, v) in fields {
        if k != "matcher" && k != "hooks" {
            out.push(
                Severity::Warning,
                "unknown-key",
                v,
                format!(
                    "unknown matcher-group key \"{k}\" is ignored{}",
                    did_you_mean(k, &["matcher", "hooks"])
                ),
            );
        }
    }
    if let Some(m) = group.get("matcher") {
        lint_matcher(event, m, out);
    }
    let Some(handlers) = group.get("hooks") else {
        out.push(
            Severity::Error,
            "missing-field",
            group,
            "matcher group has no \"hooks\" array, so nothing runs".into(),
        );
        return;
    };
    let Kind::Arr(list) = &handlers.kind else {
        out.push(
            Severity::Error,
            "invalid-structure",
            handlers,
            format!(
                "\"hooks\" must be an array of handlers, found {}",
                handlers.type_name()
            ),
        );
        return;
    };
    let matcher = group
        .get("matcher")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    for h in list {
        lint_handler(event, h, ctx, out);
        let key = format!("{matcher}\u{0}{}", fingerprint(h));
        let ev = event.to_string();
        if seen.iter().any(|(e, k)| *e == ev && *k == key) {
            out.push(
                Severity::Warning,
                "duplicate-handler",
                h,
                format!("identical handler already registered for {event} with the same matcher"),
            );
        } else {
            seen.push((ev, key));
        }
    }
}

fn fingerprint(v: &Value) -> String {
    match &v.kind {
        Kind::Null => "null".into(),
        Kind::Bool(b) => b.to_string(),
        Kind::Num(n) => n.to_string(),
        Kind::Str(s) => format!("{s:?}"),
        Kind::Arr(a) => format!(
            "[{}]",
            a.iter().map(fingerprint).collect::<Vec<_>>().join(",")
        ),
        Kind::Obj(o) => {
            let mut parts: Vec<String> = o
                .iter()
                .map(|(k, v)| format!("{k:?}:{}", fingerprint(v)))
                .collect();
            parts.sort();
            format!("{{{}}}", parts.join(","))
        }
    }
}

fn is_exact_matcher(m: &str, narrow: bool) -> bool {
    m.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || c == '_'
            || c == '|'
            || (!narrow && (c == '-' || c == ' ' || c == ','))
    })
}

fn lint_matcher(event: &str, m: &Value, out: &mut Out) {
    let Some(s) = m.as_str() else {
        out.push(
            Severity::Error,
            "invalid-matcher",
            m,
            format!("matcher must be a string, found {}", m.type_name()),
        );
        return;
    };
    if NO_MATCHER.contains(&event) {
        if !s.is_empty() && s != "*" {
            out.push(
                Severity::Warning,
                "matcher-ignored",
                m,
                format!("{event} has no matcher support; \"{s}\" is silently ignored and the hook fires every time"),
            );
        }
        return;
    }
    if s.is_empty() || s == "*" {
        return;
    }
    let narrow = event == "FileChanged" || event == "StopFailure";
    if narrow && s.chars().any(|c| c == '-' || c == ' ' || c == ',') && is_exact_matcher(s, false) {
        out.push(
            Severity::Warning,
            "matcher-regex-path",
            m,
            format!("on {event} only '|' separates alternatives; '-', ' ' or ',' push \"{s}\" onto the regex path"),
        );
    }
    if is_exact_matcher(s, narrow) {
        let tokens: Vec<&str> = s
            .split(['|', ','])
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect();
        for t in tokens {
            if TOOL_EVENTS.contains(&event) {
                if let Some(rest) = t.strip_prefix("mcp__") {
                    if !rest.contains("__") {
                        out.push(
                            Severity::Error,
                            "mcp-matcher-exact",
                            m,
                            format!("\"{t}\" is compared as an exact tool name and matches no tool; use \"{t}__.*\" to match every tool from that server"),
                        );
                    }
                    continue;
                }
                if !KNOWN_TOOLS.contains(&t) {
                    if let Some(k) = KNOWN_TOOLS.iter().find(|k| k.eq_ignore_ascii_case(t)) {
                        out.push(
                            Severity::Error,
                            "matcher-case",
                            m,
                            format!("matchers are case-sensitive: \"{t}\" matches nothing, the tool is \"{k}\""),
                        );
                    } else if let Some(k) = suggest(t, KNOWN_TOOLS) {
                        out.push(
                            Severity::Warning,
                            "matcher-unknown-tool",
                            m,
                            format!("\"{t}\" is not a built-in tool name (did you mean \"{k}\"?)"),
                        );
                    }
                }
            } else if let Some(values) = enumerated_matchers(event) {
                if !values.contains(&t) {
                    out.push(
                        Severity::Warning,
                        "matcher-unknown-value",
                        m,
                        format!(
                            "\"{t}\" is not a {event} matcher value{}; expected one of: {}",
                            did_you_mean(t, values),
                            values.join(", ")
                        ),
                    );
                }
            }
        }
    } else {
        if let Err(e) = check_regex(s) {
            out.push(
                Severity::Error,
                "invalid-matcher",
                m,
                format!("matcher \"{s}\" is not a valid regular expression: {e}"),
            );
            return;
        }
        if TOOL_EVENTS.contains(&event) && !s.starts_with('^') {
            let lead: String = s
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if KNOWN_TOOLS.contains(&lead.as_str()) {
                let others: Vec<&str> = KNOWN_TOOLS
                    .iter()
                    .copied()
                    .filter(|k| *k != lead && k.contains(lead.as_str()))
                    .collect();
                if !others.is_empty() {
                    out.push(
                        Severity::Warning,
                        "matcher-unanchored",
                        m,
                        format!(
                            "regex matchers are unanchored: \"{s}\" also matches {}; use \"^{lead}$\" for an exact match",
                            others.join(", ")
                        ),
                    );
                }
            }
        }
    }
}

/// Cheap structural check for the JavaScript regexes Claude Code uses.
fn check_regex(s: &str) -> Result<(), String> {
    let mut stack: Vec<char> = Vec::new();
    let mut chars = s.chars().peekable();
    let mut in_class = false;
    let mut prev_atom = false;
    while let Some(c) = chars.next() {
        if c == '\\' {
            if chars.next().is_none() {
                return Err("trailing backslash".into());
            }
            prev_atom = true;
            continue;
        }
        if in_class {
            if c == ']' {
                in_class = false;
                prev_atom = true;
            }
            continue;
        }
        match c {
            '[' => in_class = true,
            '(' => {
                stack.push('(');
                prev_atom = false;
                continue;
            }
            ')' => {
                if stack.pop().is_none() {
                    return Err("unmatched ')'".into());
                }
            }
            '*' | '+' | '?' => {
                if !prev_atom {
                    return Err(format!("nothing to repeat before '{c}'"));
                }
                prev_atom = false;
                continue;
            }
            '|' => {
                prev_atom = false;
                continue;
            }
            _ => {}
        }
        prev_atom = true;
    }
    if in_class {
        return Err("unterminated character class".into());
    }
    if !stack.is_empty() {
        return Err("unclosed '('".into());
    }
    Ok(())
}

fn lint_handler(event: &str, h: &Value, ctx: &Ctx, out: &mut Out) {
    let Kind::Obj(fields) = &h.kind else {
        out.push(
            Severity::Error,
            "invalid-structure",
            h,
            format!("hook handler must be an object, found {}", h.type_name()),
        );
        return;
    };
    let Some(tv) = h.get("type") else {
        out.push(
            Severity::Error,
            "missing-field",
            h,
            "handler has no \"type\" (command, http, mcp_tool, prompt or agent)".into(),
        );
        return;
    };
    let t = tv.as_str().unwrap_or("");
    if !HANDLER_TYPES.contains(&t) {
        out.push(
            Severity::Error,
            "unknown-type",
            tv,
            format!(
                "unknown handler type \"{t}\"{}",
                did_you_mean(t, HANDLER_TYPES)
            ),
        );
        return;
    }
    let allowed: Vec<&str> = COMMON_KEYS.iter().chain(type_keys(t)).copied().collect();
    for (k, v) in fields {
        if !allowed.contains(&k.as_str()) {
            out.push(
                Severity::Warning,
                "unknown-key",
                v,
                format!(
                    "\"{k}\" is not a {t} handler field and is ignored{}",
                    did_you_mean(k, &allowed)
                ),
            );
        }
    }
    if event == "SessionStart" && !(t == "command" || t == "mcp_tool") {
        out.push(
            Severity::Error,
            "unsupported-type",
            tv,
            format!("SessionStart only runs command and mcp_tool hooks; this {t} hook never runs"),
        );
    }
    if (event == "SessionStart" || event == "Setup") && t == "mcp_tool" {
        out.push(
            Severity::Warning,
            "mcp-before-servers",
            tv,
            format!("{event} fires before MCP servers connect at launch, so this mcp_tool hook is skipped then; use a command hook for launch-time work"),
        );
    }
    if let Some(cond) = h.get("if") {
        lint_if(event, cond, out);
    }
    if let Some(to) = h.get("timeout") {
        match to.kind {
            Kind::Num(n) if n > 0.0 => {
                if event == "SessionEnd" && n > 60.0 {
                    out.push(
                        Severity::Warning,
                        "timeout-capped",
                        to,
                        "SessionEnd hooks share a budget capped at 60 seconds; a longer timeout is cut short".into(),
                    );
                }
            }
            _ => out.push(
                Severity::Error,
                "invalid-timeout",
                to,
                "timeout must be a positive number of seconds".into(),
            ),
        }
    }
    if let Some(once) = h.get("once") {
        if once.kind == Kind::Bool(true) {
            out.push(
                Severity::Warning,
                "once-ignored",
                once,
                "\"once\" is only honored in skill frontmatter; in settings and plugin files the hook runs every time".into(),
            );
        }
    }
    if let Some(of) = h.get("onFailure") {
        match of.as_str() {
            Some("continue") | Some("block") => {
                let is_async = [h.get("async"), h.get("asyncRewake")]
                    .iter()
                    .any(|v| v.map(|v| v.kind == Kind::Bool(true)).unwrap_or(false));
                if is_async {
                    out.push(
                        Severity::Warning,
                        "onfailure-no-effect",
                        of,
                        "onFailure has no effect on async/asyncRewake hooks".into(),
                    );
                } else if matches!(
                    event,
                    "Stop" | "SubagentStop" | "TaskCompleted" | "TeammateIdle"
                ) {
                    out.push(
                        Severity::Warning,
                        "onfailure-no-effect",
                        of,
                        format!("onFailure has no effect on {event} hooks"),
                    );
                }
            }
            _ => out.push(
                Severity::Error,
                "invalid-onfailure",
                of,
                "onFailure must be \"continue\" or \"block\"".into(),
            ),
        }
    }
    match t {
        "command" => lint_command(event, h, ctx, out),
        "http" => lint_http(h, out),
        "mcp_tool" => {
            for f in ["server", "tool"] {
                require_string(h, f, t, out);
            }
        }
        _ => {
            if let Some(p) = require_string(h, "prompt", t, out) {
                if !p.contains("$ARGUMENTS") {
                    let at = h.get("prompt").unwrap_or(h);
                    out.push(
                        Severity::Info,
                        "prompt-no-arguments",
                        at,
                        "prompt does not use $ARGUMENTS, so the model never sees the hook input"
                            .into(),
                    );
                }
            }
        }
    }
}

fn require_string<'a>(h: &'a Value, field: &str, t: &str, out: &mut Out) -> Option<&'a str> {
    match h.get(field) {
        None => {
            out.push(
                Severity::Error,
                "missing-field",
                h,
                format!("{t} handler needs a \"{field}\""),
            );
            None
        }
        Some(v) => match v.as_str() {
            Some(s) if !s.trim().is_empty() => Some(s),
            _ => {
                out.push(
                    Severity::Error,
                    "missing-field",
                    v,
                    format!("\"{field}\" must be a non-empty string"),
                );
                None
            }
        },
    }
}

fn lint_if(event: &str, cond: &Value, out: &mut Out) {
    let Some(s) = cond.as_str() else {
        out.push(
            Severity::Error,
            "invalid-if",
            cond,
            "\"if\" must be a single permission rule string such as \"Bash(git *)\"".into(),
        );
        return;
    };
    if !TOOL_EVENTS.contains(&event) {
        out.push(
            Severity::Error,
            "if-never-runs",
            cond,
            format!(
                "\"if\" is only evaluated on tool events; on {event} a hook with \"if\" never runs"
            ),
        );
        return;
    }
    if s.contains("&&") || s.contains("||") {
        out.push(
            Severity::Error,
            "invalid-if",
            cond,
            "\"if\" holds exactly one rule; split combined conditions into separate handlers"
                .into(),
        );
        return;
    }
    let tool: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let rest = &s[tool.len()..];
    let shaped =
        !tool.is_empty() && (rest.is_empty() || (rest.starts_with('(') && rest.ends_with(')')));
    if !shaped {
        out.push(
            Severity::Error,
            "invalid-if",
            cond,
            format!("\"{s}\" is not permission rule syntax; expected Tool or Tool(pattern), e.g. \"Bash(rm *)\""),
        );
    } else if !tool.starts_with("mcp__") && !KNOWN_TOOLS.contains(&tool.as_str()) {
        if let Some(k) = KNOWN_TOOLS.iter().find(|k| k.eq_ignore_ascii_case(&tool)) {
            out.push(
                Severity::Error,
                "matcher-case",
                cond,
                format!("tool names are case-sensitive: \"{tool}\" should be \"{k}\""),
            );
        }
    }
}

fn lint_http(h: &Value, out: &mut Out) {
    if let Some(url) = require_string(h, "url", "http", out) {
        let at = h.get("url").unwrap_or(h);
        if let Some(rest) = url.strip_prefix("http://") {
            let host = rest.split(['/', ':']).next().unwrap_or("");
            if !matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0") {
                out.push(
                    Severity::Warning,
                    "http-plaintext",
                    at,
                    format!("hook input (prompts, tool arguments, file paths) is sent in clear text to {host}; use https"),
                );
            }
        } else if !url.starts_with("https://") {
            out.push(
                Severity::Error,
                "invalid-url",
                at,
                format!("\"{url}\" is not an http(s) URL"),
            );
        }
    }
    let allowed: Vec<String> = match h.get("allowedEnvVars").map(|v| &v.kind) {
        Some(Kind::Arr(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    };
    if let Some(Kind::Obj(headers)) = h.get("headers").map(|v| &v.kind) {
        for (name, v) in headers {
            let Some(val) = v.as_str() else { continue };
            for var in env_refs(val) {
                if !allowed.contains(&var) {
                    out.push(
                        Severity::Error,
                        "header-env-not-allowed",
                        v,
                        format!("header \"{name}\" uses ${var}, which is not in allowedEnvVars, so it is sent as an empty string"),
                    );
                }
            }
            if let Some(kind) = secret_kind(val) {
                out.push(
                    Severity::Error,
                    "hardcoded-secret",
                    v,
                    format!("header \"{name}\" contains what looks like a hardcoded {kind}; read it from an env var listed in allowedEnvVars"),
                );
            }
        }
    }
}

/// `$NAME` and `${NAME}` references in a string.
pub fn env_refs(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let braced = b.get(i + 1) == Some(&b'{');
            let start = if braced { i + 2 } else { i + 1 };
            let mut j = start;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j > start && !b[start].is_ascii_digit() {
                out.push(s[start..j].to_string());
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

fn secret_kind(s: &str) -> Option<&'static str> {
    let prefixes: &[(&str, &str)] = &[
        ("sk-ant-", "Anthropic API key"),
        ("sk-proj-", "OpenAI API key"),
        ("sk-", "API key"),
        ("ghp_", "GitHub token"),
        ("github_pat_", "GitHub token"),
        ("gho_", "GitHub token"),
        ("xoxb-", "Slack token"),
        ("xoxp-", "Slack token"),
        ("glpat-", "GitLab token"),
        ("AKIA", "AWS access key"),
    ];
    for word in s.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '=') {
        for (p, kind) in prefixes {
            if word.starts_with(p) && word.len() >= p.len() + 12 {
                return Some(kind);
            }
        }
    }
    let lower = s.to_ascii_lowercase();
    if let Some(rest) = lower
        .strip_prefix("bearer ")
        .or_else(|| lower.strip_prefix("basic "))
    {
        let tok = rest.trim();
        if tok.len() >= 16 && !tok.contains('$') {
            return Some("credential");
        }
    }
    None
}

fn lint_command(event: &str, h: &Value, ctx: &Ctx, out: &mut Out) {
    let Some(cmd) = require_string(h, "command", "command", out) else {
        return;
    };
    let at = h.get("command").unwrap_or(h);
    let exec_form = h.get("args").is_some();
    if let Some(args) = h.get("args") {
        let ok = matches!(&args.kind, Kind::Arr(a) if a.iter().all(|v| v.as_str().is_some()));
        if !ok {
            out.push(
                Severity::Error,
                "invalid-args",
                args,
                "\"args\" must be an array of strings".into(),
            );
        }
        if !cmd.contains('/') && !cmd.contains('\\') && cmd.trim().contains(char::is_whitespace) {
            out.push(
                Severity::Error,
                "exec-form-spaces",
                at,
                format!("with \"args\" set, \"{cmd}\" is spawned as one executable name and the spawn fails; move the extra words into args"),
            );
        }
        if let Some(sh) = h.get("shell") {
            out.push(
                Severity::Warning,
                "shell-ignored",
                sh,
                "\"shell\" is ignored when \"args\" is set (exec form)".into(),
            );
        }
    }
    if let Some(sh) = h.get("shell") {
        if !matches!(sh.as_str(), Some("bash") | Some("powershell")) {
            out.push(
                Severity::Error,
                "invalid-shell",
                sh,
                "\"shell\" must be \"bash\" or \"powershell\"".into(),
            );
        }
    }
    if ctx.source == Source::Settings && cmd.contains("CLAUDE_PLUGIN_ROOT") {
        out.push(
            Severity::Warning,
            "plugin-root-outside-plugin",
            at,
            "${CLAUDE_PLUGIN_ROOT} is only set for plugin hooks; in a settings file it expands to nothing".into(),
        );
    }
    if !exec_form {
        if has_unquoted_placeholder(cmd) {
            out.push(
                Severity::Warning,
                "unquoted-placeholder",
                at,
                "path placeholder is unquoted in shell form and breaks on paths with spaces; quote it or use exec form with \"args\"".into(),
            );
        }
        if cmd.contains("/dev/tty") {
            out.push(
                Severity::Warning,
                "dev-tty",
                at,
                "hooks run without a controlling terminal, so /dev/tty fails; return \"terminalSequence\" or \"systemMessage\" instead".into(),
            );
        }
        if pipes_to_shell(cmd) {
            out.push(
                Severity::Warning,
                "remote-exec",
                at,
                "downloads and runs remote code on every hook run".into(),
            );
        }
        let is_async = h
            .get("async")
            .map(|v| v.kind == Kind::Bool(true))
            .unwrap_or(false);
        if !is_async && backgrounds_without_redirect(cmd) {
            out.push(
                Severity::Warning,
                "background-no-redirect",
                at,
                "a background process without redirected stdout/stderr keeps the hook's output open (up to 10s, and PreToolUse may refuse the decision); add '>/dev/null 2>&1'".into(),
            );
        }
    }
    if let Some(rm) = destructive(cmd) {
        out.push(
            Severity::Warning,
            "destructive-command",
            at,
            format!("hook runs a destructive command ({rm}) automatically"),
        );
    }
    if !ctx.check_fs {
        return;
    }
    let script = script_path(cmd, h, exec_form);
    let Some((raw, direct)) = script else { return };
    let Some(path) = resolve(&raw, ctx) else {
        return;
    };
    if !path.exists() {
        out.push(
            Severity::Error,
            "missing-script",
            at,
            format!(
                "script \"{raw}\" not found at {}; the hook fails to start on every run",
                path.display()
            ),
        );
        return;
    }
    #[cfg(unix)]
    if direct {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.is_file() && meta.permissions().mode() & 0o111 == 0 {
                out.push(
                    Severity::Error,
                    "script-not-executable",
                    at,
                    format!(
                        "script \"{raw}\" is not executable; run chmod +x {}",
                        path.display()
                    ),
                );
            }
        }
    }
    #[cfg(not(unix))]
    let _ = direct;
    if event == "PreToolUse" {
        if let Ok(body) = std::fs::read_to_string(&path) {
            let blocks = body.contains("exit 2")
                || body.contains("exit(2)")
                || body.contains("permissionDecision")
                || body.contains("exit 2;");
            let exits_one = body.contains("exit 1") || body.contains("exit(1)");
            if exits_one && !blocks {
                out.push(
                    Severity::Warning,
                    "exit-1-does-not-block",
                    at,
                    format!("\"{raw}\" exits 1 but never 2 or prints a permissionDecision; exit 1 is a non-blocking error, so the tool call still runs"),
                );
            }
        }
    }
}

fn has_unquoted_placeholder(cmd: &str) -> bool {
    let mut in_dq = false;
    let mut in_sq = false;
    let b = cmd.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' if !in_sq => in_dq = !in_dq,
            b'\'' if !in_dq => in_sq = !in_sq,
            b'\\' => i += 1,
            b'$' if !in_dq && !in_sq => {
                let rest = &cmd[i..];
                for p in [
                    "${CLAUDE_PROJECT_DIR}",
                    "$CLAUDE_PROJECT_DIR",
                    "${CLAUDE_PLUGIN_ROOT}",
                    "$CLAUDE_PLUGIN_ROOT",
                    "${CLAUDE_PLUGIN_DATA}",
                    "$CLAUDE_PLUGIN_DATA",
                ] {
                    if rest.starts_with(p) {
                        return true;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

fn pipes_to_shell(cmd: &str) -> bool {
    let fetch = cmd.contains("curl ") || cmd.contains("wget ");
    if !fetch {
        return false;
    }
    cmd.split('|').skip(1).any(|seg| {
        let first = seg.split_whitespace().next().unwrap_or("");
        let first = first.trim_start_matches("sudo");
        matches!(first, "sh" | "bash" | "zsh" | "python" | "python3" | "node")
            || seg
                .split_whitespace()
                .take(2)
                .any(|w| matches!(w, "sh" | "bash"))
    })
}

fn backgrounds_without_redirect(cmd: &str) -> bool {
    let b = cmd.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c != b'&' {
            continue;
        }
        let prev = if i > 0 { b[i - 1] } else { b' ' };
        let next = b.get(i + 1).copied().unwrap_or(b' ');
        if prev == b'&' || next == b'&' || prev == b'>' || next == b'>' {
            continue;
        }
        let segment = &cmd[..i];
        let segment = segment.rsplit([';', '\n']).next().unwrap_or(segment);
        let segment = segment.rsplit("&&").next().unwrap_or(segment);
        if !segment.contains('>') {
            return true;
        }
    }
    false
}

fn destructive(cmd: &str) -> Option<&'static str> {
    let c = format!(" {} ", cmd.replace(['\n', ';', '&', '|'], " "));
    let pats: &[(&str, &str)] = &[
        (" rm -rf / ", "rm -rf /"),
        (" rm -rf ~ ", "rm -rf ~"),
        (" rm -rf $HOME ", "rm -rf $HOME"),
        (" git reset --hard ", "git reset --hard"),
        (" git clean -fdx ", "git clean -fdx"),
        (" git push --force ", "git push --force"),
        (" git push -f ", "git push -f"),
        (" mkfs", "mkfs"),
        (" dd if=", "dd"),
    ];
    pats.iter().find(|(p, _)| c.contains(p)).map(|(_, n)| *n)
}

const INTERPRETERS: &[&str] = &[
    "bash",
    "sh",
    "zsh",
    "python",
    "python3",
    "node",
    "bun",
    "deno",
    "ruby",
    "perl",
    "pwsh",
    "powershell",
    "powershell.exe",
    "uv",
    "npx",
    "tsx",
];

/// Returns the script the hook runs and whether it is executed directly
/// (needs the executable bit) rather than through an interpreter.
fn script_path(cmd: &str, h: &Value, exec_form: bool) -> Option<(String, bool)> {
    let words: Vec<String> = if exec_form {
        let mut w = vec![cmd.to_string()];
        if let Some(Kind::Arr(a)) = h.get("args").map(|v| &v.kind) {
            w.extend(a.iter().filter_map(|v| v.as_str().map(String::from)));
        }
        w
    } else {
        shell_words(cmd.lines().next().unwrap_or(""))
    };
    let mut iter = words.iter().filter(|w| {
        !w.contains('=') || w.starts_with('$') || w.starts_with('/') || w.starts_with('.')
    });
    let first = iter.next()?.clone();
    let base = first.rsplit('/').next().unwrap_or(&first).to_string();
    if INTERPRETERS.contains(&base.as_str()) {
        let script =
            iter.find(|w| !w.starts_with('-') && w.as_str() != "run" && w.as_str() != "run.")?;
        if looks_like_path(script) {
            return Some((script.clone(), false));
        }
        return None;
    }
    if looks_like_path(&first) {
        return Some((first, true));
    }
    None
}

fn looks_like_path(w: &str) -> bool {
    w.starts_with("./")
        || w.starts_with("../")
        || w.starts_with('/')
        || w.starts_with("~/")
        || w.starts_with("$CLAUDE_PROJECT_DIR")
        || w.starts_with("${CLAUDE_PROJECT_DIR}")
        || w.starts_with("$CLAUDE_PLUGIN_ROOT")
        || w.starts_with("${CLAUDE_PLUGIN_ROOT}")
        || w.starts_with(".claude/")
        || w.starts_with("hooks/")
        || w.starts_with("scripts/")
}

/// Minimal POSIX-ish word splitting: honors quotes, stops at shell operators.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut chars = line.chars().peekable();
    let (mut dq, mut sq) = (false, false);
    while let Some(c) = chars.next() {
        match c {
            '"' if !sq => {
                dq = !dq;
                has = true;
            }
            '\'' if !dq => {
                sq = !sq;
                has = true;
            }
            '\\' if !sq => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    has = true;
                }
            }
            c if c.is_whitespace() && !dq && !sq => {
                if has {
                    words.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            '|' | ';' | '&' | '>' | '<' if !dq && !sq => break,
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        words.push(cur);
    }
    words
}

fn resolve(raw: &str, ctx: &Ctx) -> Option<PathBuf> {
    let root = ctx.root.to_string_lossy().to_string();
    let mut p = raw.to_string();
    for var in ["${CLAUDE_PROJECT_DIR}", "$CLAUDE_PROJECT_DIR"] {
        if p.starts_with(var) {
            if ctx.source == Source::Plugin {
                return None;
            }
            p = p.replacen(var, &root, 1);
        }
    }
    for var in ["${CLAUDE_PLUGIN_ROOT}", "$CLAUDE_PLUGIN_ROOT"] {
        if p.starts_with(var) {
            if ctx.source == Source::Settings {
                return None;
            }
            p = p.replacen(var, &root, 1);
        }
    }
    if p.contains('$') || p.contains('*') {
        return None;
    }
    if p != raw {
        return Some(PathBuf::from(p));
    }
    if let Some(rest) = p.strip_prefix("~/") {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        return Some(Path::new(&home).join(rest));
    }
    let path = Path::new(p.strip_prefix("./").unwrap_or(&p));
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        Some(ctx.root.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_respect_quotes() {
        assert_eq!(
            shell_words("\"$CLAUDE_PROJECT_DIR\"/a b.sh --x | jq"),
            vec!["$CLAUDE_PROJECT_DIR/a", "b.sh", "--x"]
        );
    }

    #[test]
    fn env_refs_found() {
        assert_eq!(env_refs("Bearer $TOKEN ${OTHER}"), vec!["TOKEN", "OTHER"]);
    }

    #[test]
    fn regex_check() {
        assert!(check_regex("mcp__.*__write.*").is_ok());
        assert!(check_regex("(Edit|Write").is_err());
        assert!(check_regex("*Edit").is_err());
    }

    #[test]
    fn background_detection() {
        assert!(backgrounds_without_redirect("npm run dev &"));
        assert!(!backgrounds_without_redirect("npm run dev > log 2>&1 &"));
        assert!(!backgrounds_without_redirect("a && b"));
    }
}
