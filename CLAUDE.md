# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project Overview

A Rust statusline generator for Claude Code. Reads JSON from stdin, outputs an ANSI-colored statusline showing: working directory (fish-style shortened), git branch, worktree indicator, model name, context window usage (progress bar + percentage), session cost, lines changed, agent name, and session duration.

## Key Commands

```bash
cargo build --release     # Release build
cargo run < test.json     # Manual test with sample data
cargo check               # Quick type checking
cargo clippy              # Lint
cargo fmt                 # Format
cargo test                # Run unit tests
just ci                   # Run all CI checks (fmt, clippy, tests)
```

**Important**: Never generate test JSON files. Use `test.json` in the repo root or look in `~/.claude`.

## Architecture

### Entry Point

**src/main.rs**: Calls `statusline()` from the library and prints the result. No CLI argument handling.

**src/lib.rs**: All logic lives here.

### Input Structs

`StatusInput` is the top-level serde struct. All fields use `#[serde(default)]` so missing fields deserialize gracefully.

| Struct | Key Fields | Purpose |
|--------|-----------|---------|
| `StatusInput` | workspace, model, output_style, context_window, cost, worktree, agent, effort, thinking, vim, rate_limits | Top-level container |
| `Workspace` | `current_dir: Option<String>`, `git_worktree: Option<String>` | Working directory (required for meaningful output); `git_worktree` is set when cwd is in a linked git worktree |
| `Model` | `display_name: Option<String>` | Model name shown in statusline; `" (1M context)"` is stripped |
| `OutputStyle` | `name: Option<String>` | Style label (e.g. "explanatory") shown in parens |
| `ContextWindow` | `context_window_size`, `used_percentage`, `current_usage` | Context usage data; bar shows a `┊` tick at the 200k boundary when `context_window_size > 200000` |
| `CurrentUsage` | `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens` | Token breakdown for manual % calculation |
| `Cost` | `total_cost_usd`, `total_duration_ms`, `total_lines_added`, `total_lines_removed` | Session cost and line change stats |
| `Worktree` | `name` | Worktree indicator |
| `Agent` | `name: Option<String>` | Agent name when running as a sub-agent |
| `Effort` | `level: Option<String>` | Reasoning effort suffix on model name (`·max`, `·xhigh`, `·low`, `·medium`); `high` is the default and is suppressed |
| `Thinking` | `enabled: bool` | When true, appends `✻` glyph to model name |
| `Vim` | `mode: Option<String>` | Vim mode badge at the front of the line; maps `NORMAL/INSERT/VISUAL/VISUAL LINE` to `[N]/[I]/[V]/[V-L]` |
| `RateLimits` | `five_hour`, `seven_day` (each `RateLimitWindow`) | Subscriber-only; absent for API users. Shown at the end of the line |
| `RateLimitWindow` | `used_percentage: f64` | Color-coded with the same thresholds as the context bar |

### Statusline Assembly

`render()` builds these display components, then joins non-empty ones with `•` separators. The vim badge is prepended outside the bullet-joined sequence:

1. **Vim badge** (prepended): `[N]/[I]/[V]/[V-L]` colored by mode; only shown when `vim.mode` is present
2. **Path**: Fish-style shortened (`fish_shorten_path`), colored cyan
3. **Git branch**: Via a single `git status --porcelain=v2 --branch` call, colored green. Decorations append in this order: `*` (red, when dirty), `↑N` (ahead), `↓M` (behind), `↟name` (worktree, from `worktree.name` or falling back to `workspace.git_worktree`)
4. **Lines changed**: `+N -M` from cost data, green/red, glued to the branch component
5. **Model**: Nerd Font icon + model name in orange, with optional `·<effort>` suffix (`·max`, etc.; `high` suppressed), `✻` thinking glyph, and style suffix in gray
6. **Context bar**: 15-char progress bar (█/░) + percentage, color-coded via `pct_color()` (red ≥90%, orange ≥70%, yellow ≥50%, gray <50%); `┊` tick inserted at the 200k boundary when `context_window_size > 200000`
7. **Cost**: Dollar amount, color-coded (green <$5, yellow <$20, red ≥$20)
8. **Agent**: Agent name in gray with icon
9. **Duration**: Formatted as `Nh Mm` or `<1m`, from `total_duration_ms`
10. **Rate limits**: `5h NN%` and/or `7d NN%`, percentages color-coded via `pct_color()`; both windows separated by `·`

### ANSI Colors

Constants defined at the top of `lib.rs`: `RESET`, `RED`, `GREEN`, `YELLOW`, `CYAN`, `GRAY`, `ORANGE`, `LIGHT_CYAN`, `LIGHT_BLUE`, `LIGHT_MAGENTA`, `GOLD`. Uses standard ANSI escapes and 256-color codes.

### Key Functions

All internal; only `statusline()` is `pub`.

- `statusline()` — Main orchestrator, returns the assembled String
- `render(&StatusInput)` — Builds the display string from a parsed input
- `read_input()` — Reads stdin, deserializes to `StatusInput`
- `git_status(dir)` — Single `git status --porcelain=v2 --branch` call; returns `Option<GitStatus>` (None when not a repo)
- `parse_git_status(stdout)` — Pure parser for porcelain v2 output, isolated for testing
- `fish_shorten_path(path)` — Strips `$HOME` prefix to `~`, shortens intermediate dirs to first char (hidden dirs keep dot + first char)
- `pct_color(f64)` — Maps a percentage to one of red/orange/yellow/gray; shared by context bar and rate limits
- `format_cost(f64)` — 3 decimal places below $0.01, 2 above
- `format_duration(ms)` — Formats milliseconds as `Nh Mm`, `Nm`, or `<1m`

### Display Format

```
[N] path  branch*↑2↓1 ↟worktree(+N -M) • 󰊭 Model·max✻ (style) • 󱦛 ███┊░░░░░░░░░░░░ 22% • 󰊖 $7.50 • 󰚩 agent • 󰔚 15m • 5h 78% · 7d 34%
```

Components are suppressed when their data is absent: vim badge, effort suffix, thinking glyph, and rate limits all degrade to nothing when their fields aren't present in the JSON.

## Dependencies

- **serde** + **serde_json**: JSON deserialization only
- **External**: `git` (required for branch/repo detection)

## Input Format

JSON on stdin. See `test.json` for the full structure. Only `workspace.current_dir` is required for meaningful output; all other fields are optional and degrade gracefully when absent.
