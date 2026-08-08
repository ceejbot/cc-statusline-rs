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
|--------|------------|---------|
| `StatusInput` | workspace, model, output_style, context_window, cost, worktree, agent, effort, thinking, vim, rate_limits | Top-level container |
| `Workspace` | `current_dir: Option<String>`, `git_worktree: Option<String>` | Working directory (required for meaningful output); `git_worktree` is set when cwd is in a linked git worktree |
| `Model` | `display_name: Option<String>` | Model name shown in statusline; `" (1M context)"` is stripped |
| `OutputStyle` | `name: Option<String>` | Style label (e.g. "explanatory") shown in parens |
| `ContextWindow` | `context_window_size`, `used_percentage`, `current_usage` | Context usage data; bar shows a `┊` tick at the 200k boundary when `context_window_size > 200000` |
| `CurrentUsage` | `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens` | Token breakdown for manual % calculation |
| `Cost` | `total_cost_usd`, `total_duration_ms`, `total_lines_added`, `total_lines_removed` | Session cost and line change stats |
| `Worktree` | `name` | Worktree indicator |
| `Agent` | `name: Option<String>` | Agent name when running as a sub-agent |
| `Pr` | `number: Option<u64>`, `review_state: Option<String>` | Open PR badge glued to the git branch; present only inside a git repo with an open PR. `url` is ignored (not displayable) |
| `Effort` | `level: Option<String>` | Reasoning effort suffix on model name (`·max`, `·ultra`, `·xhigh`, `·low`, `·medium`; `ultra` is the Opus 4.8 level); rendered generically for any value, with `high` (the default) suppressed |
| `Thinking` | `enabled: bool` | When true, appends `✻` glyph to model name |
| `Vim` | `mode: Option<String>` | Vim mode badge at the front of the line; maps `NORMAL/INSERT/VISUAL/VISUAL LINE` to `[N]/[I]/[V]/[V-L]` |
| `RateLimits` | `five_hour`, `seven_day` (each `RateLimitWindow`) | Subscriber-only; absent for API users. Shown at the end of the line |
| `RateLimitWindow` | `used_percentage: f64`, `resets_at: Option<u64>` | `used_percentage` color-coded with the same thresholds as the context bar. `resets_at` is a Unix epoch (seconds); rendered as a relative countdown, but only when the window is ≥50% used and the reset is still in the future |
| (top-level) | `transcript_path: Option<String>` | Path to the session's JSONL transcript; feeds the cache-break indicator (mtime = last activity, tail = TTL evidence) |

### Statusline Assembly

`render()` builds these display components, then joins non-empty ones with `•` separators. The vim badge is prepended outside the bullet-joined sequence:

1. **Vim badge** (prepended): `[N]/[I]/[V]/[V-L]` colored by mode; only shown when `vim.mode` is present
2. **Path**: Fish-style shortened (`fish_shorten_path`), colored cyan
3. **Git branch**: Via a single `git status --porcelain=v2 --branch` call, colored green. Decorations append in this order: `*` (red, when dirty), `↑N` (ahead), `↓M` (behind), `↟name` (worktree, from `worktree.name` or falling back to `workspace.git_worktree`), `#N` (PR badge, from `pr.number`, colored by `pr.review_state`: approved=green, changes_requested=red, pending=yellow, draft/absent=gray)
4. **Lines changed**: `+N -M` from cost data, green/red, glued to the branch component
5. **Model**: Nerd Font icon + model name in orange, with optional `·<effort>` suffix (`·max`, etc.; `high` suppressed), `✻` thinking glyph, and style suffix in gray
6. **Context bar**: 15-char progress bar (█/░) + percentage, color-coded via `pct_color()` (red ≥90%, orange ≥70%, yellow ≥50%, gray <50%); `┊` tick inserted at the 200k boundary when `context_window_size > 200000`
7. **Cost**: Dollar amount, color-coded (green <$5, yellow <$20, red ≥$20)
8. **Agent**: Agent name in gray with icon
9. **Duration**: Formatted as `Nh Mm` or `<1m`, from `total_duration_ms`
10. **Rate limits**: `5h NN%` and/or `7d NN%`, percentages color-coded via `pct_color()`; both windows separated by `·`. Each window appends a gray reset countdown (e.g. `·2h 12m`, day-aware as `·5d 2h`) glued to its percentage, shown only when that window is ≥50% used and `resets_at` is in the future (a stale/elapsed timestamp degrades to no countdown)
11. **Cache break**: prompt-cache expiry countdown from `transcript_path`. Last activity = transcript mtime; TTL tier detected from the last nonzero `"cache_creation"` bucket in the transcript tail (`ephemeral_1h_input_tokens` → 1h, `ephemeral_5m_input_tokens` → 5m), falling back to env overrides (`FORCE_PROMPT_CACHING_5M`, `ENABLE_PROMPT_CACHING_1H`) then a plan heuristic (rate limits present → 1h, else 5m). Hidden while >25% of TTL remains; `󰔛 12m` yellow <25%, orange <10%, red <5% (seconds below 2m); `󰜗 cold` in light blue after expiry. `DISABLE_PROMPT_CACHING=1` or an unreadable transcript suppresses it entirely. Pairs with `statusLine.refreshInterval` (e.g. 10s) in settings so the countdown ticks while the session is idle

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
- `format_reset(secs)` — Day-aware rate-limit reset countdown: `Nd Nh`, `Nh Mm`, `Nm`, or `<1m` (rolls into days, unlike `format_duration`)
- `detect_ttl(tail)` — Pure parser: finds the most recent nonzero `"cache_creation"` usage object in a transcript tail and returns the `Ttl` tier (escaped in-text mentions don't match)
- `cache_status(path, has_rate_limits)` — IO shell: transcript mtime + TTL detection → `(remaining_secs, ttl_secs)`; `None` when caching is disabled or transcript unreadable
- `cache_display(remaining, ttl)` — Pure band logic for the cache-break component (hidden / yellow / orange / red / cold)
- `format_cache_countdown(secs)` — `Nm` above two minutes, `Ns` below (tuned to a 10s refresh)

### Display Format

```
[N] path  branch*↑2↓1 ↟worktree #1234(+N -M) • 󰊭 Model·ultra✻ (style) • 󱦛 ███┊░░░░░░░░░░░░ 22% • 󰊖 $7.50 • 󰚩 agent • 󰔚 15m • 5h 78%·2h 12m · 7d 34% • 󰔛 12m
```

Components are suppressed when their data is absent: vim badge, effort suffix, thinking glyph, rate limits, and the cache-break countdown all degrade to nothing when their fields aren't present in the JSON (or, for the cache indicator, while the cache is comfortably warm).

## Dependencies

- **serde** + **serde_json**: JSON deserialization only
- **External**: `git` (required for branch/repo detection)

## Input Format

JSON on stdin. See `test.json` for the full structure. Only `workspace.current_dir` is required for meaningful output; all other fields are optional and degrade gracefully when absent.

## Project memory

This project uses the trivia MCP. All memories are tagged `project:cc-statusline-rs`. Recall by that tag at the start of work. Add new lessons via the `session-retro` skill.
