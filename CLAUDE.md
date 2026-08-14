# CLAUDE.md

Guidance for Claude Code when working in this repository.

## Project Overview

A Rust statusline generator for Claude Code. Reads JSON from stdin, outputs an ANSI-colored statusline in one or two lines depending on terminal width. Line 1 (identity): working directory (fish-style shortened), git branch, worktree indicator, lines changed, model name, context window usage (progress bar + percentage). Line 2 (accounting): session cost, agent name, rate limits, the last turn's cache hit rate, and the cache clock with a projected next-message cost (warm vs cold). When everything fits the terminal comfortably on one line, both groups render as a single line.

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
| `StatusInput` | workspace, model, output_style, context_window, cost, worktree, agent, effort, vim, rate_limits | Top-level container |
| `Workspace` | `current_dir: Option<String>`, `git_worktree: Option<String>` | Working directory (required for meaningful output); `git_worktree` is set when cwd is in a linked git worktree |
| `Model` | `display_name: Option<String>` | Model name shown in statusline; `" (1M context)"` is stripped |
| `OutputStyle` | `name: Option<String>` | Style label (e.g. "explanatory") shown in parens; suppressed when it's `default` (the badge marks deviation, not the norm) |
| `ContextWindow` | `context_window_size`, `used_percentage`, `current_usage` | Context usage data; bar shows a `┊` tick at the 200k boundary when `context_window_size > 200000` |
| `CurrentUsage` | `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens` | Token breakdown for manual % calculation; also feeds the cache hit-rate indicator |
| `Cost` | `total_cost_usd`, `total_lines_added`, `total_lines_removed` | Session cost and line change stats |
| `Worktree` | `name` | Worktree indicator |
| `Agent` | `name: Option<String>` | Agent name when running as a sub-agent |
| `Pr` | `number: Option<u64>`, `review_state: Option<String>` | Open PR badge glued to the git branch; present only inside a git repo with an open PR. `url` is ignored (not displayable) |
| `Effort` | `level: Option<String>` | Reasoning effort suffix on model name (`·max`, `·ultra`, `·xhigh`, `·low`, `·medium`; `ultra` is the Opus 4.8 level); rendered generically for any value, with `high` (the default) suppressed |
| `Vim` | `mode: Option<String>` | Vim mode badge at the front of the line; maps `NORMAL/INSERT/VISUAL/VISUAL LINE` to `[N]/[I]/[V]/[V-L]` |
| `RateLimits` | `five_hour`, `seven_day` (each `RateLimitWindow`) | Subscriber-only; absent for API users. Shown at the end of the line |
| `RateLimitWindow` | `used_percentage: f64`, `resets_at: Option<u64>` | `used_percentage` color-coded with the same thresholds as the context bar. `resets_at` is a Unix epoch (seconds); rendered as a relative countdown, but only when the window is ≥50% used and the reset is still in the future |
| (top-level) | `transcript_path: Option<String>` | Path to the session's JSONL transcript; feeds the cache-break indicator (mtime = last activity, tail = TTL evidence) |

### Statusline Assembly

`render()` builds these display components, joining non-empty ones with `•` separators. Layout is dynamic: if the fully assembled single line fits the terminal width (`COLUMNS` env var, set by Claude Code since v2.1.153) with `COMFORT_MARGIN` (4 columns) of slack, everything stays on one line. Otherwise — or when `COLUMNS` is absent/unparseable — the components split: line 1 carries components 1–6 (identity: location, model, context), line 2 carries components 7–11 (accounting: money, limits, cache). Width is measured with `visible_width()`, which counts ANSI escapes as zero cells. The second line is emitted only when it has content. Claude Code renders each stdout line as its own statusline row. The vim badge is prepended outside the bullet-joined sequence:

1. **Vim badge** (prepended): `[N]/[I]/[V]/[V-L]` colored by mode; only shown when `vim.mode` is present
2. **Path**: Fish-style shortened (`fish_shorten_path`), colored cyan
3. **Git branch**: Via a single `git status --porcelain=v2 --branch` call, colored green. Decorations append in this order: `*` (red, when dirty), `↑N` (ahead), `↓M` (behind), `↟name` (worktree, from `worktree.name` or falling back to `workspace.git_worktree`), `#N` (PR badge, from `pr.number`, colored by `pr.review_state`: approved=green, changes_requested=red, pending=yellow, draft/absent=gray)
4. **Lines changed**: `+N -M` from cost data, green/red, glued to the branch component
5. **Model**: Model name in orange (no icon — the name self-identifies), with optional `·<effort>` suffix (`·max`, etc.; `high` suppressed) and style suffix in gray (suppressed for `default`)
6. **Context bar**: 15-char progress bar (█/░) + percentage, color-coded via `pct_color()` (red ≥90%, orange ≥70%, yellow ≥50%, gray <50%); `┊` tick inserted at the 200k boundary when `context_window_size > 200000`
7. **Cost**: Dollar amount, color-coded (green <$5, yellow <$20, red ≥$20)
8. **Agent**: Agent name in gray with icon
9. **Rate limits**: `5h NN%` and/or `7d NN%`, percentages color-coded via `pct_color()`; both windows separated by `·`. Each window appends a gray reset countdown (e.g. `·2h 12m`, day-aware as `·5d 2h`) glued to its percentage, shown only when that window is ≥50% used and `resets_at` is in the future (a stale/elapsed timestamp degrades to no countdown)
10. **Cache hit rate**: `󰑌 NN%` — share of the last turn's prompt served from the cache, `cache_read / (input + cache_creation + cache_read)` from `current_usage`, via `cache_hit_rate()`. Color-coded on an inverted scale via `hit_rate_color()` (green ≥90%, yellow ≥50%, orange ≥20%, red below): a warm turn sits in the high 90s, a cache-busting turn (model/effort switch, `/compact`, MCP change) shows as a red single-digit for one turn. Suppressed when `current_usage` is absent or all counters are zero
11. **Cache break**: the local wall-clock time the prompt cache will expire (e.g. `󰔛 3:42pm`), from `transcript_path`. Last activity = transcript mtime; TTL tier detected from the last nonzero `"cache_creation"` bucket in the transcript tail (`ephemeral_1h_input_tokens` → 1h, `ephemeral_5m_input_tokens` → 5m), falling back to env overrides (`FORCE_PROMPT_CACHING_5M`, `ENABLE_PROMPT_CACHING_1H`) then a plan heuristic (rate limits present → 1h, else 5m). Always visible (an absolute time stays true even when the statusline isn't refreshing): gray while >25% of TTL remains, yellow <25%, orange <10%, red <5%; `󰜗 cold` in light blue after expiry. `DISABLE_PROMPT_CACHING=1` or an unreadable transcript suppresses it entirely. Pairs with `statusLine.refreshInterval` (e.g. 10s) in settings so the color bands and `cold` flip stay current while the session is idle. **Next-message cost projection** glued on: while warm `·20¢→$4.00` (context re-read at the warm cache-read rate → rebuild at the cold cache-write rate), after expiry just `·$4.00`. The currently-true figure renders gold (the line's money color); the looming one stays gray — emphasis flips when the cache goes cold. Input-side only (output tokens are unknowable); context tokens from `current_usage` (fallback: `used_percentage` × window); rate = family base input price (Fable/Mythos $10, Opus $5, Sonnet $3, Haiku $1 per MTok, matched on `display_name`) × 0.1 warm, × 1.25 (5m tier) or 2.0 (1h tier) cold. Unknown model family suppresses the projection, not the clock

### ANSI Colors

Constants defined at the top of `lib.rs`: `RESET`, `RED`, `GREEN`, `YELLOW`, `CYAN`, `GRAY`, `ORANGE`, `LIGHT_CYAN`, `LIGHT_BLUE`, `LIGHT_MAGENTA`, `GOLD`. Uses standard ANSI escapes and 256-color codes.

### Key Functions

All internal; only `statusline()` is `pub`.

- `statusline()` — Main orchestrator, returns the assembled String
- `render(&StatusInput)` — Thin wrapper: reads `COLUMNS` via `terminal_cols()` and delegates
- `render_with_width(&StatusInput, Option<usize>)` — Builds the display string from a parsed input; the width parameter decides single-line vs split layout (None → split), kept explicit for tests
- `visible_width(&str)` — On-screen column count: ANSI CSI escapes are zero width, all other chars one cell
- `terminal_cols()` — Parses the `COLUMNS` env var
- `read_input()` — Reads stdin, deserializes to `StatusInput`
- `git_status(dir)` — Single `git status --porcelain=v2 --branch` call; returns `Option<GitStatus>` (None when not a repo)
- `parse_git_status(stdout)` — Pure parser for porcelain v2 output, isolated for testing
- `fish_shorten_path(path)` — Strips `$HOME` prefix to `~`, shortens intermediate dirs to first char (hidden dirs keep dot + first char)
- `pct_color(f64)` — Maps a percentage to one of red/orange/yellow/gray; shared by context bar and rate limits
- `hit_rate_color(f64)` — Inverse scale for the cache hit rate (high = good): green ≥90, yellow ≥50, orange ≥20, red below
- `cache_hit_rate(usage)` — Share of the last turn's prompt served from cache as a percentage; `None` when all counters are zero
- `format_cost(f64)` — 3 decimal places below $0.01, 2 above
- `format_reset(secs)` — Day-aware rate-limit reset countdown: `Nd Nh`, `Nh Mm`, `Nm`, or `<1m`
- `detect_ttl(tail)` — Pure parser: finds the most recent nonzero `"cache_creation"` usage object in a transcript tail and returns the `Ttl` tier (escaped in-text mentions don't match)
- `cache_status(path, has_rate_limits)` — IO shell: transcript mtime + TTL detection → `(remaining_secs, ttl_secs)`; `None` when caching is disabled or transcript unreadable
- `cache_display(remaining, ttl, break_at, projection)` — Pure band logic for the cache-break component (gray / yellow / orange / red / cold), wrapping the preformatted break time; appends the `(warm, cold)` projection as `·warm→cold` while warm, `·cold` once expired; the currently-true figure is gold, the other gray
- `format_break_time(epoch_secs, tz)` — Epoch → compact local wall-clock time (`3:42pm`) via jiff; takes the timezone as a parameter so tests can pin a fixed offset
- `base_input_price(display_name)` — Family-matched base input $/MTok (fable/mythos 10, opus 5, sonnet 3, haiku 1); `None` for unknown families
- `context_tokens(ctx)` — Tokens in the context window from `current_usage`, falling back to `used_percentage` × window size
- `project_next_cost(tokens, base, ttl_secs)` — Pure math: `(warm, cold)` input-side next-message cost; warm = 0.1×, cold = 1.25× (5m) or 2× (1h)
- `format_money(usd)` — `<1¢` / `20¢` / `$4.00`; rounds to integer cents first so branch and display agree at the dollar boundary

### Display Format

```
[N] path  branch*↑2↓1 ↟worktree #1234(+N -M) • Model·ultra (style) • 󱦛 ███┊░░░░░░░░░░░░ 22%
󰊖 $7.50 • 󰚩 agent • 5h 78%·2h 12m · 7d 34% • 󰑌 98% • 󰔛 3:42pm ·20¢→$4.00
```

Claude Code renders each stdout line as a separate statusline row. The split happens only when the single line wouldn't fit `COLUMNS` minus the comfort margin; in a wide terminal the same components render as one bullet-joined line. Components are suppressed when their data is absent — or when they'd merely confirm the norm: vim badge, effort suffix (`high`), style suffix (`default`), rate limits, cache hit rate, cache-break time, and the cost projection all degrade to nothing; the entire second line disappears when it has no components.

## Dependencies

- **serde** + **serde_json**: JSON deserialization only
- **jiff**: local-timezone formatting for the cache-break time
- **External**: `git` (required for branch/repo detection)

## Input Format

JSON on stdin. See `test.json` for the full structure. Only `workspace.current_dir` is required for meaningful output; all other fields are optional and degrade gracefully when absent.

## Project memory

This project uses the trivia MCP. All memories are tagged `project:cc-statusline-rs`. Recall by that tag at the start of work. Add new lessons via the `session-retro` skill.
