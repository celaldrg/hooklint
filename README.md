# hooklint

**Lint Claude Code hooks before they silently fail.**

Claude Code hooks are shell commands, HTTP calls, MCP tools and prompts that run on events such as `PreToolUse`, `PostToolUse` and `SessionStart`. They are a favourite place to put guardrails: block `rm -rf`, keep secrets out of edits, format on save.

The trouble is that most hook mistakes produce **no error at all**. A matcher written as `bash` matches nothing because matchers are case-sensitive. `mcp__github` is compared as an exact tool name and never fires. A guard script that exits `1` instead of `2` lets the tool call through. A matcher on `Stop` is ignored, an `if` on a non-tool event means the hook never runs, and a misspelled event name is simply skipped. You find out when the guardrail you relied on was never there.

`hooklint` reads `.claude/settings.json`, `.claude/settings.local.json`, `~/.claude/settings.json` and plugin `hooks/hooks.json` files and tells you, with line and column, which hooks will not do what you think.

- One small static binary, zero dependencies, written in Rust.
- Checks the configuration against the current hooks reference: 33 events, 5 handler types, matcher rules per event.
- Opens the scripts your hooks point to: missing files, missing `chmod +x`, and `PreToolUse` guards that never exit 2.
- `--json` for CI and editors, `--strict` to fail on warnings.

## Install

```sh
cargo install --git https://github.com/celaldrg/hooklint
```

Or download the Linux x86_64 binary from the [latest release](https://github.com/celaldrg/hooklint/releases/latest).

## Usage

```text
hooklint [OPTIONS] [PATH...]

PATH is a settings.json / settings.local.json / hooks.json file or a project
directory (default: .). In a directory hooklint reads .claude/settings.json,
.claude/settings.local.json and hooks/hooks.json (plugins).

    --user        also lint ~/.claude/settings.json
    --json        machine-readable output
    --strict      exit 1 on warnings too
    --quiet       only show errors
    --no-fs       skip checks that read script files on disk
    --no-color    disable colors
```

Exit codes: `0` clean, `1` problems found, `2` usage or read error.

## Example

[`examples/broken`](examples/broken/.claude/settings.json) is a realistic project settings file. Every hook in it looks plausible:

```text
$ hooklint examples/broken
examples/broken/.claude/settings.json
     5:20  error    matchers are case-sensitive: "bash" matches nothing, the tool is "Bash"  matcher-case
     7:43  warning  path placeholder is unquoted in shell form and breaks on paths with spaces; quote it or use exec form with "args"  unquoted-placeholder
     7:43  warning  "$CLAUDE_PROJECT_DIR/.claude/hooks/block-rm.sh" exits 1 but never 2 or prints a permissionDecision; exit 1 is a non-blocking error, so the tool call still runs  exit-1-does-not-block
    11:20  warning  regex matchers are unanchored: "Edit.*" also matches MultiEdit, NotebookEdit; use "^Edit$" for an exact match  matcher-unanchored
    13:43  error    script "$CLAUDE_PROJECT_DIR/.claude/hooks/guard-secrets.sh" not found at examples/broken/.claude/hooks/guard-secrets.sh; the hook fails to start on every run  missing-script
   13:111  warning  "timout" is not a command handler field and is ignored (did you mean "timeout"?)  unknown-key
    17:20  error    "mcp__github" is compared as an exact tool name and matches no tool; use "mcp__github__.*" to match every tool from that server  mcp-matcher-exact
    19:36  warning  hook input (prompts, tool arguments, file paths) is sent in clear text to hooks.example.com; use https  http-plaintext
    20:43  error    header "Authorization" uses $AUDIT_TOKEN, which is not in allowedEnvVars, so it is sent as an empty string  header-env-not-allowed
    28:75  error    "if" holds exactly one rule; split combined conditions into separate handlers  invalid-if
    34:20  warning  Stop has no matcher support; "Bash" is silently ignored and the hook fires every time  matcher-ignored
    36:43  warning  hooks run without a controlling terminal, so /dev/tty fails; return "terminalSequence" or "systemMessage" instead  dev-tty
    36:43  error    script "./scripts/notify.sh" not found at examples/broken/scripts/notify.sh; the hook fails to start on every run  missing-script
    42:20  warning  "start" is not a SessionStart matcher value; expected one of: startup, resume, clear, compact, fork  matcher-unknown-value
    44:21  error    SessionStart only runs command and mcp_tool hooks; this prompt hook never runs  unsupported-type
    44:41  info     prompt does not use $ARGUMENTS, so the model never sees the hook input  prompt-no-arguments
    48:28  error    "UserPromptSubmitted" is not a hook event, so these hooks never run (did you mean "UserPromptSubmit"?)  unknown-event

8 errors, 8 warnings, 1 note in 1 file
```

[`examples/clean`](examples/clean/.claude/settings.json) is the same intent written correctly:

```text
$ hooklint examples/clean
ok examples/clean/.claude/settings.json
0 errors, 0 warnings, 0 notes in 1 file
```

## What it checks

| Area | Rule | Severity |
| --- | --- | --- |
| File | `invalid-json` (with position; Claude Code skips the whole file), `invalid-structure`, `hooks-disabled` (`disableAllHooks: true`) | error / warning |
| Events | `unknown-event` with a did-you-mean suggestion, `empty-event` | error / info |
| Matchers | `matcher-case` (`bash` vs `Bash`), `mcp-matcher-exact` (`mcp__server` without `__.*`), `matcher-ignored` on events with no matcher support, `matcher-unknown-value` (`SessionStart`, `PreCompact`, `Notification`, `StopFailure`...), `matcher-unknown-tool`, `matcher-unanchored` (`Edit.*` also hits `NotebookEdit`), `matcher-regex-path`, `invalid-matcher` (broken regex) | error / warning |
| Handlers | `unknown-type`, `unknown-key` (typos like `timout`), `missing-field`, `unsupported-type` (only command and mcp_tool run on `SessionStart`), `mcp-before-servers`, `invalid-timeout`, `timeout-capped` (`SessionEnd`), `once-ignored`, `invalid-onfailure`, `onfailure-no-effect`, `duplicate-handler` | error / warning |
| `if` | `if-never-runs` (only tool events evaluate it), `invalid-if` (`&&`, `\|\|`, bad syntax), tool-name case | error |
| Commands | `exec-form-spaces` (`"node script.js"` with `args`), `invalid-args`, `invalid-shell`, `shell-ignored`, `unquoted-placeholder`, `plugin-root-outside-plugin`, `dev-tty`, `background-no-redirect`, `remote-exec` (`curl ... \| sh`), `destructive-command` | error / warning |
| Scripts | `missing-script`, `script-not-executable`, `exit-1-does-not-block` | error / warning |
| HTTP | `header-env-not-allowed` (variable not in `allowedEnvVars` is sent empty), `hardcoded-secret`, `http-plaintext`, `invalid-url` | error / warning |
| Prompt and agent | `missing-field`, `prompt-no-arguments` | error / info |

Script checks resolve `${CLAUDE_PROJECT_DIR}` to the project root (the folder that holds `.claude/`) and `${CLAUDE_PLUGIN_ROOT}` to the plugin root (the folder that holds `hooks/`). Use `--no-fs` to lint a file on its own.

## Use it in CI

```yaml
- run: cargo install --git https://github.com/celaldrg/hooklint
- run: hooklint --strict .
```

or as a git pre-commit hook:

```sh
#!/bin/sh
exec hooklint --quiet .
```

## Limits

- The regex check is structural (balanced groups, classes, nothing-to-repeat). It is not a full JavaScript regex engine.
- Script analysis is a heuristic: it reads the script the command starts and looks for `exit 2` or a `permissionDecision`. Scripts that compute their exit code dynamically may produce a false positive; the rule is a warning for that reason.
- Hooks inside skill and subagent frontmatter are not linted yet.

The rules follow the public [Claude Code hooks reference](https://docs.claude.com/en/docs/claude-code/hooks). hooklint is an independent project and is not affiliated with Anthropic.

## Related

- [ctxlint](https://github.com/celaldrg/ctxlint): lint AGENTS.md, CLAUDE.md and .cursorrules
- [subagentlint](https://github.com/celaldrg/subagentlint): lint Claude Code subagent files
- [skillcheck](https://github.com/celaldrg/skillcheck): lint Agent Skills folders
- [mcpaudit](https://github.com/celaldrg/mcpaudit): audit MCP client configs for secrets and unpinned servers

## License

[MIT](LICENSE)
