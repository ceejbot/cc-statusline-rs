use std::io::{self, Read};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

// ANSI color constants
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";
const ORANGE: &str = "\x1b[38;5;208m";
const LIGHT_CYAN: &str = "\x1b[38;5;14m";
const LIGHT_BLUE: &str = "\x1b[38;5;12m";
const LIGHT_MAGENTA: &str = "\x1b[38;5;13m";
const GOLD: &str = "\x1b[38;5;3m";

// Typed serde structs for JSON input.
// Container-level `#[serde(default)]` fills any missing field from the struct's
// `Default`, so partial/empty JSON deserializes gracefully without per-field
// attrs.
#[derive(Deserialize, Default)]
#[serde(default)]
struct StatusInput {
    workspace: Workspace,
    model: Model,
    output_style: OutputStyle,
    context_window: Option<ContextWindow>,
    cost: Option<Cost>,
    worktree: Option<Worktree>,
    agent: Option<Agent>,
    effort: Option<Effort>,
    thinking: Option<Thinking>,
    vim: Option<Vim>,
    rate_limits: Option<RateLimits>,
    pr: Option<Pr>,
    transcript_path: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Workspace {
    current_dir: Option<String>,
    git_worktree: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Model {
    display_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OutputStyle {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ContextWindow {
    context_window_size: u64,
    used_percentage: Option<f64>,
    current_usage: Option<CurrentUsage>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CurrentUsage {
    input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Cost {
    total_cost_usd: f64,
    total_duration_ms: Option<u64>,
    total_lines_added: u64,
    total_lines_removed: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Worktree {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Agent {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Pr {
    number: Option<u64>,
    review_state: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Effort {
    level: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Thinking {
    enabled: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Vim {
    mode: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RateLimits {
    five_hour: Option<RateLimitWindow>,
    seven_day: Option<RateLimitWindow>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RateLimitWindow {
    used_percentage: f64,
    resets_at: Option<u64>, // Unix epoch seconds; absent until the window has usage
}

/// Read the Claude Code status-hook JSON from stdin and return the assembled
/// ANSI statusline. Malformed or empty input degrades to defaults rather than
/// failing — a statusline should never error out of a prompt.
pub fn statusline() -> String {
    let input = read_input().unwrap_or_default();
    render(&input)
}

/// Point the Claude Code `statusLine` setting at this binary: merge the entry
/// into `~/.claude/settings.json`, creating the file if needed and preserving
/// everything else in it. Returns the success message to print.
pub fn setup(command_override: Option<&str>) -> Result<String, String> {
    let command = match command_override {
        Some(c) => c.to_string(),
        // Deliberately not canonicalized: a symlink like
        // /opt/homebrew/bin/cc-statusline-rs survives upgrades, while the
        // Cellar path it points at does not.
        None => std::env::current_exe()
            .map_err(|e| format!("cannot locate this executable: {e}"))?
            .to_string_lossy()
            .into_owned(),
    };

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "neither HOME nor USERPROFILE is set; cannot find ~/.claude".to_string())?;
    let claude_dir = std::path::Path::new(&home).join(".claude");
    std::fs::create_dir_all(&claude_dir).map_err(|e| format!("cannot create {}: {e}", claude_dir.display()))?;
    let settings = claude_dir.join("settings.json");

    let existing = match std::fs::read_to_string(&settings) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("cannot read {}: {e}", settings.display())),
    };
    let verb = if existing.is_some() { "updated" } else { "created" };
    let merged = merge_statusline(existing.as_deref(), &command).map_err(|e| format!("{}: {e}", settings.display()))?;

    // Write to a sibling temp file and rename so a failure mid-write can
    // never leave settings.json truncated.
    let tmp = claude_dir.join("settings.json.tmp");
    std::fs::write(&tmp, merged).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &settings).map_err(|e| format!("cannot replace {}: {e}", settings.display()))?;

    Ok(format!("{verb} {} — statusLine command: {command}", settings.display()))
}

/// Pure merge: replace the `statusLine` key in the settings JSON, leaving all
/// sibling keys untouched. `existing` is the raw file contents, or None when
/// the file doesn't exist yet. A file that doesn't parse as a JSON object is
/// an error, never clobbered.
fn merge_statusline(existing: Option<&str>, command: &str) -> Result<String, String> {
    let mut root = match existing.map(str::trim).filter(|text| !text.is_empty()) {
        None => serde_json::Value::Object(serde_json::Map::new()),
        Some(text) => serde_json::from_str(text).map_err(|e| format!("not valid JSON ({e}); leaving it alone"))?,
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "top level is not a JSON object; leaving it alone".to_string())?;
    obj.insert(
        "statusLine".to_string(),
        serde_json::json!({
            "type": "command",
            "command": command,
            "refreshInterval": 10,
        }),
    );
    let mut out = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

fn model_display(input: &StatusInput) -> String {
    let Some(ref model) = input.model.display_name else {
        return String::new();
    };
    let model = model.replace(" (1M context)", "");
    let effort_suffix = input
        .effort
        .as_ref()
        .and_then(|e| e.level.as_deref())
        .filter(|&l| l != "high")
        .map(|l| format!("·{l}"))
        .unwrap_or_default();
    let thinking_glyph = if input.thinking.as_ref().is_some_and(|t| t.enabled) {
        "✻"
    } else {
        ""
    };
    let style_suffix = match input.output_style.name {
        Some(ref style) => format!(" {GRAY}({style}){RESET}"),
        None => String::new(),
    };
    format!("{LIGHT_CYAN}\u{e26d} {ORANGE}{model}{effort_suffix}{thinking_glyph}{style_suffix}")
}

fn context_display(ctx: &ContextWindow) -> String {
    // Use API-provided percentage when available, fall back to manual calculation
    let pct = if let Some(api_pct) = ctx.used_percentage {
        api_pct.min(100.0)
    } else {
        let window_size = ctx.context_window_size;
        let used = ctx
            .current_usage
            .as_ref()
            .map(|u| u.input_tokens + u.cache_creation_input_tokens + u.cache_read_input_tokens)
            .unwrap_or(0);
        if window_size > 0 {
            ((used as f64 * 100.0) / window_size as f64).min(100.0)
        } else {
            0.0
        }
    };

    let bar_width: usize = 15;
    let filled = ((pct * bar_width as f64 / 100.0).round() as usize).min(bar_width);
    // Tick the 200k boundary when the window is larger than 200k.
    let tick_at = (ctx.context_window_size > 200_000)
        .then(|| ((200_000.0 * bar_width as f64) / ctx.context_window_size as f64).round() as usize);
    let mut bar = String::new();
    for i in 0..bar_width {
        if Some(i) == tick_at {
            bar.push('┊');
        }
        bar.push(if i < filled { '\u{2588}' } else { '\u{2591}' });
    }

    format!(
        "{LIGHT_MAGENTA}\u{f49b} {GRAY}{bar}{RESET} {}{}%{RESET}",
        pct_color(pct),
        pct.round() as u32
    )
}

/// Slack required beyond the line's visible width before we trust a single
/// line to fit: a line that just barely fits reads as cramped, and terminal
/// wrap at the exact edge looks worse than the deliberate break.
const COMFORT_MARGIN: usize = 4;

/// Columns the string occupies on screen: ANSI escape sequences are zero
/// width, every other char is one cell. The glyphs used here (Nerd Font
/// icons, box-drawing, arrows) all render single-cell.
fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI sequence: ESC [ params, terminated by a byte in @..~
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            width += 1;
        }
    }
    width
}

/// Terminal width as Claude Code reports it (the COLUMNS env var, set for
/// statusline processes since v2.1.153).
fn terminal_cols() -> Option<usize> {
    std::env::var("COLUMNS").ok()?.parse().ok()
}

fn render(input: &StatusInput) -> String {
    render_with_width(input, terminal_cols())
}

fn render_with_width(input: &StatusInput, cols: Option<usize>) -> String {
    let vim_display = input
        .vim
        .as_ref()
        .and_then(|v| v.mode.as_deref())
        .filter(|m| !m.is_empty())
        .map(|m| {
            let (label, color) = match m {
                "NORMAL" => ("N", GREEN),
                "INSERT" => ("I", ORANGE),
                "VISUAL" => ("V", LIGHT_MAGENTA),
                "VISUAL LINE" => ("V-L", LIGHT_MAGENTA),
                other => (other, GRAY),
            };
            format!("{color}[{label}]{RESET} ")
        })
        .unwrap_or_default();

    let now = epoch_now();

    let rate_limits_display = input
        .rate_limits
        .as_ref()
        .map(|rl| {
            let mut parts = Vec::new();
            for (label, win) in [("5h", &rl.five_hour), ("7d", &rl.seven_day)] {
                if let Some(w) = win {
                    // Surface the reset countdown only once a window starts to
                    // matter (>=50%, the first non-gray pct_color band) and only
                    // when resets_at is present and still in the future.
                    let reset = w
                        .resets_at
                        .filter(|_| w.used_percentage >= 50.0)
                        .map(|at| at.saturating_sub(now))
                        .filter(|&remaining| remaining > 0)
                        .map(|remaining| format!("{GRAY}·{}{RESET}", format_reset(remaining)))
                        .unwrap_or_default();
                    parts.push(format!(
                        "{GRAY}{label} {}{}%{RESET}{reset}",
                        pct_color(w.used_percentage),
                        w.used_percentage.round() as u32
                    ));
                }
            }
            parts.join(&format!(" {GRAY}·{RESET} "))
        })
        .unwrap_or_default();

    let cache_break_display = input
        .transcript_path
        .as_deref()
        .and_then(|path| cache_status(path, input.rate_limits.is_some()))
        .and_then(|(remaining, ttl)| {
            let break_at = format_break_time(now + remaining, &jiff::tz::TimeZone::system());
            let projection = input
                .model
                .display_name
                .as_deref()
                .and_then(base_input_price)
                .and_then(|base| {
                    let tokens = input.context_window.as_ref().and_then(context_tokens)?;
                    let (warm, cold) = project_next_cost(tokens, base, ttl);
                    Some((format_money(warm), format_money(cold)))
                });
            cache_display(remaining, ttl, &break_at, projection)
        })
        .unwrap_or_default();

    let current_dir = match input.workspace.current_dir {
        Some(ref dir) => dir.as_str(),
        None => return format!("{RED}\u{f071} missing workspace.current_dir{RESET}"),
    };

    let branch_display = git_status(current_dir).map(|status| {
        let mut s = format!("{GREEN}{}{RESET}", status.branch);
        if status.dirty {
            s.push_str(&format!("{RED}*{RESET}"));
        }
        if status.ahead > 0 {
            s.push_str(&format!(" {GRAY}\u{2191}{}{RESET}", status.ahead));
        }
        if status.behind > 0 {
            s.push_str(&format!(" {GRAY}\u{2193}{}{RESET}", status.behind));
        }
        let worktree_name = input
            .worktree
            .as_ref()
            .and_then(|w| w.name.as_deref())
            .or(input.workspace.git_worktree.as_deref())
            .filter(|n| !n.is_empty());
        if let Some(n) = worktree_name {
            s.push_str(&format!(" {GRAY}\u{219f}{n}{RESET}"));
        }
        if let Some(number) = input.pr.as_ref().and_then(|p| p.number) {
            let color = match input.pr.as_ref().and_then(|p| p.review_state.as_deref()) {
                Some("approved") => GREEN,
                Some("changes_requested") => RED,
                Some("pending") => YELLOW,
                _ => GRAY, // draft, or absent review_state
            };
            s.push_str(&format!(" {color}\u{f407}#{number}{RESET}"));
        }
        s
    });

    let display_dir = fish_shorten_path(current_dir);

    let lines_changed = if let Some(ref cost) = input.cost {
        let added = cost.total_lines_added;
        let removed = cost.total_lines_removed;
        if added > 0 || removed > 0 {
            format!("({GREEN}+{added}{RESET} {RED}-{removed}{RESET})")
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let cost_display = if let Some(ref cost) = input.cost {
        let total = cost.total_cost_usd;
        if total > 0.0 {
            let formatted = format_cost(total);
            let cost_color = if total < 5.0 {
                GREEN
            } else if total < 20.0 {
                YELLOW
            } else {
                RED
            };
            format!("{GOLD}\u{f155} {cost_color}{formatted}{RESET}")
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let agent_display = input
        .agent
        .as_ref()
        .and_then(|a| a.name.as_deref())
        .filter(|n| !n.is_empty())
        .map(|n| format!("{LIGHT_CYAN}\u{f06a9} {GRAY}{n}{RESET}"))
        .unwrap_or_default();

    let duration_display = input
        .cost
        .as_ref()
        .and_then(|c| c.total_duration_ms)
        .filter(|&ms| ms > 0)
        .map(|ms| {
            let formatted = format_duration(ms);
            format!("{GRAY}\u{f0150} {formatted}{RESET}")
        })
        .unwrap_or_default();

    let sep = format!(" {GRAY}• {RESET}");

    // Line 1 is identity: where you are and what the model is doing.
    let line1: Vec<String> = [
        model_display(input),
        input.context_window.as_ref().map(context_display).unwrap_or_default(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();

    // Line 2 is accounting: money, time, limits, and the cache clock. Long
    // branch names crowd line 1, so the whole block lives below it.
    let line2: Vec<String> = [
        cost_display, agent_display, duration_display, rate_limits_display, cache_break_display,
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect();

    let head = match branch_display {
        Some(branch) => {
            format!("{vim_display}{CYAN}{display_dir}{RESET} {LIGHT_BLUE}\u{f02a2}{RESET} {branch}{lines_changed}")
        }
        None => format!("{vim_display}{CYAN}{display_dir}{RESET}"),
    };

    let join = |parts: &[String]| {
        if parts.is_empty() {
            String::new()
        } else {
            format!(" {GRAY}• {RESET}{}", parts.join(&sep))
        }
    };

    // Prefer a single line when the terminal comfortably fits everything;
    // fall back to the two-line split otherwise, or when the width is
    // unknown (the deliberate break degrades better than an edge-wrapped
    // line).
    let combined: Vec<String> = line1.iter().chain(line2.iter()).cloned().collect();
    let single = format!("{head}{}", join(&combined));
    if line2.is_empty() {
        return single;
    }
    if let Some(cols) = cols {
        if visible_width(&single) + COMFORT_MARGIN <= cols {
            return single;
        }
    }
    format!("{head}{}\n{}", join(&line1), line2.join(&sep))
}

fn epoch_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn pct_color(pct: f64) -> &'static str {
    if pct >= 90.0 {
        RED
    } else if pct >= 70.0 {
        ORANGE
    } else if pct >= 50.0 {
        YELLOW
    } else {
        GRAY
    }
}

fn read_input() -> Result<StatusInput, Box<dyn std::error::Error>> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    Ok(serde_json::from_str(&buffer)?)
}

#[derive(Debug, Default, PartialEq)]
struct GitStatus {
    branch: String,
    dirty: bool,
    ahead: u32,
    behind: u32,
}

fn git_status(dir: &str) -> Option<GitStatus> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(parse_git_status(&stdout))
}

fn parse_git_status(stdout: &str) -> GitStatus {
    let mut status = GitStatus::default();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            status.branch = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let mut parts = rest.split_whitespace();
            if let Some(a) = parts.next() {
                status.ahead = a.strip_prefix('+').and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            if let Some(b) = parts.next() {
                status.behind = b.strip_prefix('-').and_then(|s| s.parse().ok()).unwrap_or(0);
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            status.dirty = true;
        }
    }
    status
}

/// Display-only home prefix: normalized to forward slashes to match
/// `fish_shorten_path`'s separator handling. Not for filesystem access.
fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(|h| h.replace('\\', "/"))
        .unwrap_or_else(|_| "/".to_string())
}

fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("{:.3}", cost)
    } else {
        format!("{:.2}", cost)
    }
}

/// Base input price in dollars per million tokens, matched by model family
/// name. An estimate: pricing is per-model, but families share a rate and the
/// statusline only receives a display name. Unknown families get no
/// projection rather than a wrong one.
fn base_input_price(display_name: &str) -> Option<f64> {
    let name = display_name.to_lowercase();
    if name.contains("fable") || name.contains("mythos") {
        Some(10.0)
    } else if name.contains("opus") {
        Some(5.0)
    } else if name.contains("sonnet") {
        Some(3.0)
    } else if name.contains("haiku") {
        Some(1.0)
    } else {
        None
    }
}

/// Tokens currently in the context window, from the last API response's usage
/// breakdown, falling back to the API-reported percentage of the window.
fn context_tokens(ctx: &ContextWindow) -> Option<u64> {
    ctx.current_usage
        .as_ref()
        .map(|u| u.input_tokens + u.cache_creation_input_tokens + u.cache_read_input_tokens)
        .filter(|&t| t > 0)
        .or_else(|| {
            ctx.used_percentage
                .filter(|_| ctx.context_window_size > 0)
                .map(|pct| (pct / 100.0 * ctx.context_window_size as f64) as u64)
        })
        .filter(|&t| t > 0)
}

/// Input-side cost of the next message as `(warm, cold)`: re-reading the
/// context from a warm cache (0.1x base input price) vs rewriting it after
/// expiry (1.25x for the 5-minute tier, 2x for the 1-hour tier). Output
/// tokens are unknowable in advance and deliberately excluded.
fn project_next_cost(tokens: u64, base_per_mtok: f64, ttl_secs: u64) -> (f64, f64) {
    let mtok = tokens as f64 / 1_000_000.0;
    let write_mult = if ttl_secs >= 3600 { 2.0 } else { 1.25 };
    (mtok * base_per_mtok * 0.1, mtok * base_per_mtok * write_mult)
}

/// Compact money formatting for the projection: cents below a dollar
/// (`20¢`), dollars above (`$4.00`), `<1¢` for dust. Rounds to integer cents
/// first so the branch and the displayed value always agree.
fn format_money(usd: f64) -> String {
    let cents = (usd * 100.0).round() as u64;
    if cents == 0 {
        "<1¢".to_string()
    } else if cents < 100 {
        format!("{cents}¢")
    } else {
        format!("${}.{:02}", cents / 100, cents % 100)
    }
}

fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "<1m".to_string()
    }
}

/// Day-aware countdown for a rate-limit window reset. Unlike `format_duration`
/// (which renders `167h 0m` for multi-day spans), this rolls into days so the
/// 7-day window reads as e.g. `2d 5h`.
fn format_reset(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "<1m".to_string()
    }
}

/// Prompt-cache TTL tier in effect for the session. The Anthropic prompt
/// cache is a sliding window: each request refreshes it, and it expires this
/// long after the last one.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ttl {
    FiveMin,
    OneHour,
}

impl Ttl {
    fn secs(self) -> u64 {
        match self {
            Ttl::FiveMin => 300,
            Ttl::OneHour => 3600,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CacheCreation {
    ephemeral_5m_input_tokens: u64,
    ephemeral_1h_input_tokens: u64,
}

/// Detect the TTL tier from a transcript tail by finding the most recent
/// `"cache_creation"` usage object with a nonzero bucket. Occurrences of the
/// phrase inside message text are quote-escaped in the JSONL (`\"...\"`), so
/// the raw-quote needle only matches real usage fields.
fn detect_ttl(tail: &str) -> Option<Ttl> {
    let needle = "\"cache_creation\":";
    let mut end = tail.len();
    while let Some(pos) = tail[..end].rfind(needle) {
        end = pos;
        let rest = &tail[pos + needle.len()..];
        // The value must be a small flat object right after the colon.
        let Some(open) = rest.find('{').filter(|&o| rest[..o].trim().is_empty()) else {
            continue;
        };
        let Some(close) = rest[open..].find('}') else {
            continue;
        };
        if let Ok(cc) = serde_json::from_str::<CacheCreation>(&rest[open..=open + close]) {
            if cc.ephemeral_1h_input_tokens > 0 {
                return Some(Ttl::OneHour);
            }
            if cc.ephemeral_5m_input_tokens > 0 {
                return Some(Ttl::FiveMin);
            }
        }
    }
    None
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1" || v == "true")
}

fn read_tail(path: &str, max_bytes: u64) -> Option<String> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max_bytes))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Compute `(remaining_secs, ttl_secs)` for the prompt cache. Last activity
/// is the transcript file's mtime; the TTL tier comes from transcript
/// evidence, then env overrides, then a plan heuristic (rate limits are only
/// sent for subscribers, who get the 1-hour cache). None means no indicator:
/// caching disabled or transcript unreadable.
fn cache_status(transcript_path: &str, has_rate_limits: bool) -> Option<(u64, u64)> {
    if env_flag("DISABLE_PROMPT_CACHING") {
        return None;
    }
    let mtime = std::fs::metadata(transcript_path).ok()?.modified().ok()?;
    // A future mtime (clock skew) degrades to zero idle time.
    let idle = SystemTime::now().duration_since(mtime).unwrap_or_default().as_secs();
    let ttl = read_tail(transcript_path, 128 * 1024)
        .and_then(|tail| detect_ttl(&tail))
        .or_else(|| {
            if env_flag("FORCE_PROMPT_CACHING_5M") {
                Some(Ttl::FiveMin)
            } else if env_flag("ENABLE_PROMPT_CACHING_1H") {
                Some(Ttl::OneHour)
            } else {
                None
            }
        })
        .unwrap_or(if has_rate_limits { Ttl::OneHour } else { Ttl::FiveMin });
    Some((ttl.secs().saturating_sub(idle), ttl.secs()))
}

/// Render the cache-break indicator: the local wall-clock time the prompt
/// cache will expire. Always visible, because the statusline mostly refreshes
/// on activity — a countdown that only appears near expiry is never seen.
/// Color escalates from gray through yellow/orange/red below 25%/10%/5% of
/// TTL remaining; a snowflake `cold` once the cache has expired (the next
/// message pays a cache rewrite).
///
/// `projection` is the preformatted `(warm, cold)` next-message cost. While
/// warm it renders as `·warm→cold` — what the next message costs now vs what
/// it will cost after expiry. Once cold, only the cold figure remains.
fn cache_display(
    remaining_secs: u64,
    ttl_secs: u64,
    break_at: &str,
    projection: Option<(String, String)>,
) -> Option<String> {
    if ttl_secs == 0 {
        return None;
    }
    if remaining_secs == 0 {
        let cost = projection
            .map(|(_, cold)| format!(" {GRAY}·{cold}{RESET}"))
            .unwrap_or_default();
        return Some(format!("{LIGHT_BLUE}\u{f0717} cold{RESET}{cost}"));
    }
    let color = if remaining_secs * 4 >= ttl_secs {
        GRAY
    } else if remaining_secs * 10 >= ttl_secs {
        YELLOW
    } else if remaining_secs * 20 >= ttl_secs {
        ORANGE
    } else {
        RED
    };
    let cost = projection
        .map(|(warm, cold)| format!(" {GRAY}·{warm}→{cold}{RESET}"))
        .unwrap_or_default();
    Some(format!("{color}\u{f051b} {break_at}{RESET}{cost}"))
}

/// Format an epoch timestamp as a compact local wall-clock time, e.g.
/// `3:42pm`. An unrepresentable timestamp degrades to an empty string.
fn format_break_time(epoch_secs: u64, tz: &jiff::tz::TimeZone) -> String {
    let Ok(ts) = jiff::Timestamp::from_second(epoch_secs.min(i64::MAX as u64) as i64) else {
        return String::new();
    };
    let zoned = ts.to_zoned(tz.clone());
    jiff::fmt::strtime::format("%-I:%M%P", &zoned).unwrap_or_default()
}

fn fish_shorten_path(path: &str) -> String {
    let home = home_dir();
    // Windows paths arrive with backslashes; display everything with forward
    // slashes so one shortening pass serves both. Identity on unix paths.
    let path = path.replace('\\', "/");
    let path = path
        .strip_prefix(&home)
        .filter(|_| !home.is_empty() && home != "/")
        // Guard the component boundary: /Users/ceejbackup must not become
        // ~backup just because it shares a string prefix with /Users/ceej.
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
        .map(|rest| format!("~{rest}"))
        .unwrap_or(path);

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 1 {
        return path;
    }

    let shortened: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            // Drive letters stay whole: C:/tools/thing → C:/t/thing.
            if i == parts.len() - 1 || part.is_empty() || *part == "~" || part.ends_with(':') {
                part.to_string()
            } else if part.starts_with('.') && part.len() > 1 {
                format!(".{}", part.chars().nth(1).unwrap_or_default())
            } else {
                part.chars().next().map(|c| c.to_string()).unwrap_or_default()
            }
        })
        .collect();

    shortened.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- format_cost ---

    #[test]
    fn format_cost_zero() {
        assert_eq!(format_cost(0.0), "0.000");
    }

    #[test]
    fn format_cost_below_threshold() {
        assert_eq!(format_cost(0.001), "0.001");
        assert_eq!(format_cost(0.005), "0.005");
        assert_eq!(format_cost(0.009), "0.009");
    }

    #[test]
    fn format_cost_at_threshold() {
        assert_eq!(format_cost(0.01), "0.01");
    }

    #[test]
    fn format_cost_above_threshold() {
        assert_eq!(format_cost(0.99), "0.99");
        assert_eq!(format_cost(19.99), "19.99");
    }

    // --- format_duration ---

    #[test]
    fn format_duration_under_one_minute() {
        assert_eq!(format_duration(0), "<1m");
        assert_eq!(format_duration(500), "<1m");
        assert_eq!(format_duration(59_999), "<1m");
    }

    #[test]
    fn format_duration_minutes_only() {
        assert_eq!(format_duration(60_000), "1m");
        assert_eq!(format_duration(945_000), "15m");
        assert_eq!(format_duration(3_599_000), "59m");
    }

    #[test]
    fn format_duration_hours_and_minutes() {
        assert_eq!(format_duration(3_600_000), "1h 0m");
        assert_eq!(format_duration(5_400_000), "1h 30m");
        assert_eq!(format_duration(7_200_000), "2h 0m");
    }

    // --- format_reset ---

    #[test]
    fn format_reset_under_one_minute() {
        assert_eq!(format_reset(0), "<1m");
        assert_eq!(format_reset(59), "<1m");
    }

    #[test]
    fn format_reset_minutes_only() {
        assert_eq!(format_reset(90), "1m");
        assert_eq!(format_reset(3_540), "59m");
    }

    #[test]
    fn format_reset_hours_and_minutes() {
        assert_eq!(format_reset(3_600 + 1_800), "1h 30m");
        assert_eq!(format_reset(4 * 3_600 + 12 * 60), "4h 12m");
    }

    #[test]
    fn format_reset_days_and_hours() {
        assert_eq!(format_reset(2 * 86_400 + 5 * 3_600), "2d 5h");
        assert_eq!(format_reset(6 * 86_400 + 23 * 3_600), "6d 23h");
    }

    // --- fish_shorten_path ---

    #[test]
    fn fish_shorten_replaces_home() {
        let home = home_dir();
        let path = format!("{home}/projects/myrepo");
        assert_eq!(fish_shorten_path(&path), "~/p/myrepo");
    }

    #[test]
    fn fish_shorten_intermediate_dirs() {
        // Use a path that won't have $HOME in it
        assert_eq!(fish_shorten_path("/usr/local/bin/tool"), "/u/l/b/tool");
    }

    #[test]
    fn fish_shorten_last_component_kept() {
        assert_eq!(fish_shorten_path("/some/deep/nested/directory"), "/s/d/n/directory");
    }

    #[test]
    fn fish_shorten_hidden_dirs() {
        assert_eq!(fish_shorten_path("/home/.config/nvim"), "/h/.c/nvim");
    }

    #[test]
    fn fish_shorten_no_slashes() {
        assert_eq!(fish_shorten_path("justfile"), "justfile");
    }

    #[test]
    fn fish_shorten_root() {
        assert_eq!(fish_shorten_path("/"), "/");
    }

    #[test]
    fn fish_shorten_tilde_preserved() {
        assert_eq!(fish_shorten_path("~/code/project"), "~/c/project");
    }

    #[test]
    fn fish_shorten_home_sibling_not_collapsed() {
        // /Users/ceejbackup shares a string prefix with /Users/ceej but is not
        // inside home; it must not render with a tilde.
        let home = home_dir();
        let path = format!("{home}extra/project");
        let shortened = fish_shorten_path(&path);
        assert!(!shortened.starts_with('~'));
        assert!(shortened.ends_with("project"));
    }

    #[test]
    fn fish_shorten_windows_drive_letter() {
        // Drive letter stays whole; separators normalize for display.
        assert_eq!(fish_shorten_path(r"C:\tools\thing"), "C:/t/thing");
    }

    #[test]
    fn fish_shorten_backslash_home() {
        // A backslash rendering of $HOME still collapses to a tilde.
        let home = home_dir();
        let path = format!("{home}/projects/myrepo").replace('/', "\\");
        assert_eq!(fish_shorten_path(&path), "~/p/myrepo");
    }

    #[test]
    fn fish_shorten_unc_path() {
        // Leading empty components survive, so UNC roots keep their slashes.
        assert_eq!(fish_shorten_path(r"\\server\share\docs\file"), "//s/s/d/file");
    }

    // --- detect_ttl ---

    #[test]
    fn detect_ttl_one_hour() {
        let tail = r#"{"usage":{"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":4058}}}"#;
        assert_eq!(detect_ttl(tail), Some(Ttl::OneHour));
    }

    #[test]
    fn detect_ttl_five_min() {
        let tail = r#"{"usage":{"cache_creation":{"ephemeral_1h_input_tokens":0,"ephemeral_5m_input_tokens":200}}}"#;
        assert_eq!(detect_ttl(tail), Some(Ttl::FiveMin));
    }

    #[test]
    fn detect_ttl_uses_most_recent_nonzero() {
        // Last occurrence has zero buckets (pure cache read); the earlier
        // nonzero one decides.
        let tail = concat!(
            r#"{"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":99}}"#,
            "\n",
            r#"{"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0}}"#,
        );
        assert_eq!(detect_ttl(tail), Some(Ttl::OneHour));
    }

    #[test]
    fn detect_ttl_absent() {
        assert_eq!(detect_ttl(r#"{"message":{"role":"user"}}"#), None);
        assert_eq!(detect_ttl(""), None);
    }

    #[test]
    fn detect_ttl_ignores_escaped_text_mentions() {
        // The phrase appearing inside message text is quote-escaped in JSONL
        // and must not match.
        let tail = r#"{"text":"transcripts record \"cache_creation\":{\"ephemeral_1h_input_tokens\":4058}"}"#;
        assert_eq!(detect_ttl(tail), None);
    }

    #[test]
    fn detect_ttl_malformed_object() {
        assert_eq!(detect_ttl(r#""cache_creation": [1,2,3]"#), None);
        assert_eq!(detect_ttl(r#""cache_creation":{"unterminated"#), None);
    }

    // --- cache_display ---

    #[test]
    fn cache_display_gray_while_warm() {
        let warm = cache_display(3600, 3600, "3:42pm", None).expect("shown while warm");
        assert!(warm.contains(GRAY) && warm.contains("3:42pm"));
        let boundary = cache_display(900, 3600, "3:42pm", None).expect("shown at exactly 25%");
        assert!(boundary.contains(GRAY));
    }

    #[test]
    fn cache_display_bands_one_hour() {
        let yellow = cache_display(899, 3600, "3:42pm", None).expect("shown below 25%");
        assert!(yellow.contains(YELLOW) && yellow.contains("3:42pm"));
        let orange = cache_display(300, 3600, "3:42pm", None).expect("shown below 10%");
        assert!(orange.contains(ORANGE));
        let red = cache_display(100, 3600, "3:42pm", None).expect("shown below 5%");
        assert!(red.contains(RED));
    }

    #[test]
    fn cache_display_bands_five_min() {
        let yellow = cache_display(74, 300, "3:42pm", None).expect("shown below 25%");
        assert!(yellow.contains(YELLOW));
        let red = cache_display(10, 300, "3:42pm", None).expect("shown below 5%");
        assert!(red.contains(RED));
    }

    #[test]
    fn cache_display_cold_after_expiry() {
        let cold = cache_display(0, 3600, "3:42pm", None).expect("cold state shown");
        assert!(cold.contains(LIGHT_BLUE) && cold.contains("cold"));
        // The break time is in the past once cold; it must not be shown.
        assert!(!cold.contains("3:42pm"));
    }

    #[test]
    fn cache_display_zero_ttl_hidden() {
        assert!(cache_display(0, 0, "3:42pm", None).is_none());
    }

    #[test]
    fn cache_display_projection_warm_shows_both() {
        let warm = cache_display(3600, 3600, "3:42pm", Some(("20¢".into(), "$4.00".into()))).expect("shown while warm");
        assert!(warm.contains("·20¢→$4.00"));
        assert!(warm.contains("3:42pm"));
    }

    #[test]
    fn cache_display_projection_cold_shows_rebuild_only() {
        let cold = cache_display(0, 3600, "3:42pm", Some(("20¢".into(), "$4.00".into()))).expect("cold state shown");
        assert!(cold.contains("·$4.00"));
        assert!(!cold.contains("20¢"));
        assert!(!cold.contains('→'));
    }

    // --- next-message cost projection ---

    #[test]
    fn base_input_price_by_family() {
        assert_eq!(base_input_price("Fable 5"), Some(10.0));
        assert_eq!(base_input_price("Claude Mythos 5"), Some(10.0));
        assert_eq!(base_input_price("Opus 4.5 (1M context)"), Some(5.0));
        assert_eq!(base_input_price("Sonnet 5"), Some(3.0));
        assert_eq!(base_input_price("Haiku 4.5"), Some(1.0));
        assert_eq!(base_input_price("Mystery Model"), None);
    }

    #[test]
    fn context_tokens_prefers_usage_breakdown() {
        let ctx: ContextWindow = serde_json::from_str(
            r#"{"context_window_size": 1000000, "used_percentage": 50.0,
                "current_usage": {"input_tokens": 100000, "cache_creation_input_tokens": 50000, "cache_read_input_tokens": 50000}}"#,
        )
        .expect("ctx deserializes");
        assert_eq!(context_tokens(&ctx), Some(200_000));
    }

    #[test]
    fn context_tokens_falls_back_to_percentage() {
        let ctx: ContextWindow = serde_json::from_str(r#"{"context_window_size": 1000000, "used_percentage": 20.0}"#)
            .expect("ctx deserializes");
        assert_eq!(context_tokens(&ctx), Some(200_000));
    }

    #[test]
    fn context_tokens_absent_when_no_data() {
        let ctx: ContextWindow = serde_json::from_str(r#"{"context_window_size": 1000000}"#).expect("ctx deserializes");
        assert_eq!(context_tokens(&ctx), None);
    }

    #[test]
    fn project_next_cost_one_hour_tier() {
        // 200k tokens on Fable 5 ($10/MTok), 1h cache: warm 0.1x, cold 2x.
        let (warm, cold) = project_next_cost(200_000, 10.0, 3600);
        assert!((warm - 0.20).abs() < 1e-9);
        assert!((cold - 4.00).abs() < 1e-9);
    }

    #[test]
    fn project_next_cost_five_min_tier() {
        // 100k tokens on Sonnet ($3/MTok), 5m cache: warm 0.1x, cold 1.25x.
        let (warm, cold) = project_next_cost(100_000, 3.0, 300);
        assert!((warm - 0.03).abs() < 1e-9);
        assert!((cold - 0.375).abs() < 1e-9);
    }

    #[test]
    fn format_money_bands() {
        assert_eq!(format_money(0.0), "<1¢");
        assert_eq!(format_money(0.004), "<1¢");
        assert_eq!(format_money(0.03), "3¢");
        assert_eq!(format_money(0.20), "20¢");
        assert_eq!(format_money(0.994), "99¢");
        assert_eq!(format_money(0.996), "$1.00");
        assert_eq!(format_money(4.0), "$4.00");
        assert_eq!(format_money(12.345), "$12.35");
    }

    // --- format_break_time ---

    fn fixed_tz(hours: i8) -> jiff::tz::TimeZone {
        jiff::tz::TimeZone::fixed(jiff::tz::Offset::constant(hours))
    }

    #[test]
    fn break_time_formats_local_wall_clock() {
        // 2026-08-08 22:42:00 UTC → 3:42pm at UTC-7.
        assert_eq!(format_break_time(1_786_228_920, &fixed_tz(-7)), "3:42pm");
    }

    #[test]
    fn break_time_morning_and_midnight() {
        // 2026-08-08 16:05:00 UTC → 9:05am at UTC-7.
        assert_eq!(format_break_time(1_786_205_100, &fixed_tz(-7)), "9:05am");
        // 2026-08-08 07:10:00 UTC → 12:10am at UTC-7 (12-hour clock, no 0 o'clock).
        assert_eq!(format_break_time(1_786_173_000, &fixed_tz(-7)), "12:10am");
    }

    // --- StatusInput deserialization ---

    #[test]
    fn deserialize_empty_json() {
        let input: StatusInput = serde_json::from_str("{}").expect("empty JSON should deserialize");
        assert!(input.workspace.current_dir.is_none());
        assert!(input.model.display_name.is_none());
        assert!(input.context_window.is_none());
        assert!(input.cost.is_none());
        assert!(input.worktree.is_none());
        assert!(input.agent.is_none());
    }

    #[test]
    fn deserialize_partial_json() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "model": {"display_name": "Opus"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("partial JSON should deserialize");
        assert_eq!(input.workspace.current_dir.as_deref(), Some("/tmp"));
        assert_eq!(input.model.display_name.as_deref(), Some("Opus"));
        assert!(input.cost.is_none());
    }

    #[test]
    fn deserialize_full_json() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp/repo"},
            "model": {"display_name": "Sonnet"},
            "output_style": {"name": "concise"},
            "context_window": {"context_window_size": 200000, "used_percentage": 42.5},
            "cost": {"total_cost_usd": 3.50, "total_duration_ms": 120000, "total_lines_added": 10, "total_lines_removed": 5},
            "worktree": {"name": "feat", "branch": "feat-branch"},
            "agent": {"name": "reviewer"},
            "pr": {"number": 99, "url": "https://example.com/pull/99", "review_state": "pending"}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("full JSON should deserialize");
        assert_eq!(input.model.display_name.as_deref(), Some("Sonnet"));
        assert_eq!(
            input
                .context_window
                .as_ref()
                .expect("context_window present")
                .used_percentage,
            Some(42.5)
        );
        assert_eq!(input.cost.as_ref().expect("cost present").total_cost_usd, 3.50);
        assert_eq!(
            input.cost.as_ref().expect("cost present").total_duration_ms,
            Some(120000)
        );
        assert_eq!(
            input.worktree.as_ref().expect("worktree present").name.as_deref(),
            Some("feat")
        );
        assert_eq!(
            input.agent.as_ref().expect("agent present").name.as_deref(),
            Some("reviewer")
        );
        let pr = input.pr.as_ref().expect("pr present");
        assert_eq!(pr.number, Some(99));
        assert_eq!(pr.review_state.as_deref(), Some("pending"));
    }

    #[test]
    fn deserialize_ignores_unknown_fields() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "unknown_field": 42}"#;
        let input: StatusInput = serde_json::from_str(json).expect("JSON with unknown fields should deserialize");
        assert_eq!(input.workspace.current_dir.as_deref(), Some("/tmp"));
    }

    // --- git_status ---

    #[test]
    fn git_status_none_for_non_repo() {
        assert!(git_status("/tmp").is_none());
    }

    #[test]
    fn parse_git_status_clean_with_upstream() {
        let stdout = "\
# branch.oid abcdef0
# branch.head main
# branch.upstream origin/main
# branch.ab +0 -0
";
        let s = parse_git_status(stdout);
        assert_eq!(s.branch, "main");
        assert!(!s.dirty);
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }

    #[test]
    fn parse_git_status_dirty_with_ahead_behind() {
        let stdout = "\
# branch.oid abcdef0
# branch.head feat
# branch.upstream origin/feat
# branch.ab +2 -1
1 .M N... 100644 100644 100644 aaa bbb file.txt
? untracked.txt
";
        let s = parse_git_status(stdout);
        assert_eq!(s.branch, "feat");
        assert!(s.dirty);
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
    }

    #[test]
    fn parse_git_status_no_upstream() {
        let stdout = "\
# branch.oid abcdef0
# branch.head new-branch
";
        let s = parse_git_status(stdout);
        assert_eq!(s.branch, "new-branch");
        assert!(!s.dirty);
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }

    #[test]
    fn parse_git_status_detached_head() {
        let stdout = "\
# branch.oid abcdef0
# branch.head (detached)
";
        let s = parse_git_status(stdout);
        assert_eq!(s.branch, "(detached)");
        assert!(!s.dirty);
    }

    #[test]
    fn parse_git_status_untracked_only_is_dirty() {
        let stdout = "\
# branch.oid abcdef0
# branch.head main
# branch.upstream origin/main
# branch.ab +0 -0
? new.txt
";
        let s = parse_git_status(stdout);
        assert!(s.dirty);
    }

    // --- statusline integration tests ---

    #[test]
    fn statusline_missing_workspace() {
        let input = StatusInput::default();
        let output = render(&input);
        assert!(output.contains("missing workspace.current_dir"));
    }

    #[test]
    fn statusline_non_git_dir() {
        let input: StatusInput = serde_json::from_str(r#"{"workspace": {"current_dir": "/tmp"}}"#)
            .expect("non-git dir JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("/tmp"));
        // No branch indicator for non-git dirs
        assert!(!output.contains("\u{f02a2}"));
    }

    #[test]
    fn statusline_with_model() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "model": {"display_name": "Opus"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("model JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("Opus"));
    }

    #[test]
    fn statusline_with_style() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "output_style": {"name": "concise"}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("style JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("Opus"));
        assert!(output.contains("concise"));
    }

    #[test]
    fn statusline_with_cost() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "cost": {"total_cost_usd": 3.50, "total_duration_ms": 120000, "total_lines_added": 10, "total_lines_removed": 5}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("cost JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("3.50"));
        assert!(output.contains("2m"));
        // lines_changed only shown next to git branch; /tmp is not a git repo
        // so +10 -5 won't appear in output for non-git dirs
    }

    #[test]
    fn statusline_lines_changed_with_git() {
        // Use this repo's own directory as a known git repo
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(
            r#"{{"workspace": {{"current_dir": "{this_dir}"}}, "cost": {{"total_cost_usd": 1.00, "total_lines_added": 10, "total_lines_removed": 5}}}}"#
        );
        let input: StatusInput = serde_json::from_str(&json).expect("git repo JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("+10"));
        assert!(output.contains("-5"));
    }

    #[test]
    fn statusline_with_agent() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "agent": {"name": "code-reviewer"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("agent JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("code-reviewer"));
    }

    #[test]
    fn statusline_context_color_red() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {"context_window_size": 200000, "used_percentage": 95.0}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("high context JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("95%"));
        assert!(output.contains(RED));
    }

    #[test]
    fn statusline_context_color_gray() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {"context_window_size": 200000, "used_percentage": 20.0}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("low context JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("20%"));
        // Gray is used for low percentages — check the percentage is colored gray
        // The output has the pattern: {pct_color}20%{RESET}
        let pct_idx = output.find("20%").expect("output should contain 20%");
        let preceding = &output[..pct_idx];
        assert!(preceding.ends_with(GRAY));
    }

    #[test]
    fn statusline_context_fallback_calculation() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {
                "context_window_size": 100000,
                "current_usage": {"input_tokens": 30000, "cache_creation_input_tokens": 10000, "cache_read_input_tokens": 10000}
            }
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("fallback context JSON should deserialize");
        let output = render(&input);
        // (30000+10000+10000)/100000 = 50%
        assert!(output.contains("50%"));
    }

    #[test]
    fn statusline_zero_cost_hidden() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "cost": {"total_cost_usd": 0.0}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("zero cost JSON should deserialize");
        let output = render(&input);
        // The dollar sign icon should not appear for zero cost
        assert!(!output.contains("\u{f155}"));
    }

    #[test]
    fn statusline_empty_agent_hidden() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "agent": {"name": ""}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("empty agent JSON should deserialize");
        let output = render(&input);
        // Agent icon should not appear for empty name
        assert!(!output.contains("\u{f06a9}"));
    }

    // --- dynamic layout ---

    fn layout_input() -> StatusInput {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "context_window": {"context_window_size": 200000, "used_percentage": 20.0},
            "cost": {"total_cost_usd": 3.50, "total_duration_ms": 120000}
        }"#;
        serde_json::from_str(json).expect("JSON deserializes")
    }

    #[test]
    fn statusline_splits_accounting_when_width_unknown() {
        let output = render_with_width(&layout_input(), None);
        let (first, second) = output.split_once('\n').expect("output has two lines");
        assert!(first.contains("Opus"));
        assert!(first.contains("20%"));
        assert!(second.contains("3.50"));
        assert!(second.contains("2m"));
    }

    #[test]
    fn statusline_single_line_in_wide_terminal() {
        let output = render_with_width(&layout_input(), Some(400));
        assert!(!output.contains('\n'));
        assert!(output.contains("Opus"));
        assert!(output.contains("3.50"));
    }

    #[test]
    fn statusline_splits_in_narrow_terminal() {
        let output = render_with_width(&layout_input(), Some(40));
        assert!(output.contains('\n'));
    }

    #[test]
    fn statusline_layout_respects_comfort_margin() {
        let input = layout_input();
        let single = render_with_width(&input, Some(400));
        let width = visible_width(&single);
        // Exactly at width + margin: still a single line.
        assert!(!render_with_width(&input, Some(width + COMFORT_MARGIN)).contains('\n'));
        // One column tighter: split.
        assert!(render_with_width(&input, Some(width + COMFORT_MARGIN - 1)).contains('\n'));
    }

    #[test]
    fn statusline_single_line_without_accounting_data() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("JSON deserializes");
        // No accounting components: one line regardless of width.
        assert!(!render_with_width(&input, None).contains('\n'));
        assert!(!render_with_width(&input, Some(40)).contains('\n'));
    }

    #[test]
    fn visible_width_ignores_ansi_escapes() {
        assert_eq!(visible_width("abc"), 3);
        assert_eq!(visible_width(&format!("{RED}abc{RESET}")), 3);
        assert_eq!(visible_width(&format!("{ORANGE}x{RESET} {GRAY}y{RESET}")), 3);
        assert_eq!(visible_width(""), 0);
        // Nerd Font glyph + text, wrapped in 256-color escapes.
        assert_eq!(visible_width(&format!("{LIGHT_MAGENTA}\u{f49b} 22%{RESET}")), 5);
    }

    // --- effort, thinking ---

    #[test]
    fn statusline_effort_max_shows_suffix() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "effort": {"level": "max"}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("effort JSON deserializes");
        let output = render(&input);
        assert!(output.contains("Opus·max"));
    }

    #[test]
    fn statusline_effort_high_suppressed() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "effort": {"level": "high"}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("effort JSON deserializes");
        let output = render(&input);
        assert!(!output.contains("·high"));
        assert!(output.contains("Opus"));
    }

    #[test]
    fn statusline_thinking_glyph_when_enabled() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "thinking": {"enabled": true}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("thinking JSON deserializes");
        let output = render(&input);
        assert!(output.contains("✻"));
    }

    #[test]
    fn statusline_thinking_glyph_hidden_when_disabled() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "thinking": {"enabled": false}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("thinking-disabled JSON deserializes");
        let output = render(&input);
        assert!(!output.contains("✻"));
    }

    #[test]
    fn statusline_strips_one_million_context_suffix() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus 4.7 (1M context)"}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("model-with-suffix JSON deserializes");
        let output = render(&input);
        assert!(output.contains("Opus 4.7"));
        assert!(!output.contains("(1M context)"));
    }

    // --- vim mode ---

    #[test]
    fn statusline_vim_normal_renders_n() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "vim": {"mode": "NORMAL"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("vim JSON deserializes");
        let output = render(&input);
        assert!(output.contains("[N]"));
    }

    #[test]
    fn statusline_vim_insert_renders_i() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "vim": {"mode": "INSERT"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("vim JSON deserializes");
        let output = render(&input);
        assert!(output.contains("[I]"));
    }

    #[test]
    fn statusline_vim_absent_renders_nothing() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("no-vim JSON deserializes");
        let output = render(&input);
        assert!(!output.contains("[N]"));
        assert!(!output.contains("[I]"));
    }

    // --- rate limits ---

    #[test]
    fn statusline_rate_limits_both_windows() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "rate_limits": {
                "five_hour": {"used_percentage": 78.5},
                "seven_day": {"used_percentage": 34}
            }
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("rate-limits JSON deserializes");
        let output = render(&input);
        assert!(output.contains("5h"));
        assert!(output.contains("79%"));
        assert!(output.contains("7d"));
        assert!(output.contains("34%"));
    }

    #[test]
    fn statusline_rate_limits_countdown_shown_when_high() {
        // >=50% with a future resets_at (~2h12m out) → countdown appears. The +30s
        // buffer keeps the minute field stable against the seconds elapsed between
        // computing the timestamp and render() reading the clock.
        let resets_at = epoch_now() + 2 * 3_600 + 12 * 60 + 30;
        let json = format!(
            r#"{{
                "workspace": {{"current_dir": "/tmp"}},
                "rate_limits": {{"five_hour": {{"used_percentage": 78.5, "resets_at": {resets_at}}}}}
            }}"#
        );
        let input: StatusInput = serde_json::from_str(&json).expect("rate-limits JSON deserializes");
        let output = render(&input);
        assert!(output.contains("2h 12m"));
    }

    #[test]
    fn statusline_rate_limits_countdown_gated_below_threshold() {
        // <50% with a future resets_at → countdown suppressed.
        let resets_at = epoch_now() + 2 * 3_600;
        let json = format!(
            r#"{{
                "workspace": {{"current_dir": "/tmp"}},
                "rate_limits": {{"seven_day": {{"used_percentage": 34, "resets_at": {resets_at}}}}}
            }}"#
        );
        let input: StatusInput = serde_json::from_str(&json).expect("rate-limits JSON deserializes");
        let output = render(&input);
        assert!(output.contains("34%"));
        assert!(!output.contains("2h"));
    }

    #[test]
    fn statusline_rate_limits_countdown_omitted_when_past() {
        // >=50% but resets_at already elapsed → countdown omitted.
        let resets_at = epoch_now().saturating_sub(3_600);
        let json = format!(
            r#"{{
                "workspace": {{"current_dir": "/tmp"}},
                "rate_limits": {{"five_hour": {{"used_percentage": 88, "resets_at": {resets_at}}}}}
            }}"#
        );
        let input: StatusInput = serde_json::from_str(&json).expect("rate-limits JSON deserializes");
        let output = render(&input);
        assert!(output.contains("88%"));
        // The countdown renders as `<pct>%{RESET}{GRAY}·<dur>`; that signature must
        // be absent when the window has already reset.
        assert!(!output.contains(&format!("%{RESET}{GRAY}\u{b7}")));
    }

    #[test]
    fn statusline_rate_limits_absent_renders_nothing() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("no-rate-limits JSON deserializes");
        let output = render(&input);
        assert!(!output.contains("5h"));
        assert!(!output.contains("7d"));
    }

    // --- 200k tick ---

    #[test]
    fn statusline_200k_tick_on_one_million_window() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {"context_window_size": 1000000, "used_percentage": 22}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("1M-window JSON deserializes");
        let output = render(&input);
        assert!(output.contains('┊'));
    }

    #[test]
    fn statusline_no_tick_on_200k_window() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {"context_window_size": 200000, "used_percentage": 50}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("200k-window JSON deserializes");
        let output = render(&input);
        assert!(!output.contains('┊'));
    }

    // --- pr badge ---

    #[test]
    fn statusline_pr_badge_approved_green() {
        // The badge is glued to the git branch, so use this repo's dir (a git repo).
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(
            r#"{{"workspace": {{"current_dir": "{this_dir}"}}, "pr": {{"number": 1234, "review_state": "approved"}}}}"#
        );
        let input: StatusInput = serde_json::from_str(&json).expect("pr JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("#1234"));
        let idx = output.find("\u{f407}").expect("output should contain pr glyph");
        assert!(output[..idx].ends_with(GREEN));
    }

    #[test]
    fn statusline_pr_badge_changes_requested_red() {
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(
            r#"{{"workspace": {{"current_dir": "{this_dir}"}}, "pr": {{"number": 7, "review_state": "changes_requested"}}}}"#
        );
        let input: StatusInput = serde_json::from_str(&json).expect("pr JSON should deserialize");
        let output = render(&input);
        assert!(output.contains("#7"));
        let idx = output.find("\u{f407}").expect("output should contain pr glyph");
        assert!(output[..idx].ends_with(RED));
    }

    #[test]
    fn statusline_pr_badge_absent_renders_nothing() {
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(r#"{{"workspace": {{"current_dir": "{this_dir}"}}}}"#);
        let input: StatusInput = serde_json::from_str(&json).expect("no-pr JSON should deserialize");
        let output = render(&input);
        assert!(!output.contains("\u{f407}"));
    }

    #[test]
    fn statusline_pr_badge_no_number_hidden() {
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(r#"{{"workspace": {{"current_dir": "{this_dir}"}}, "pr": {{"review_state": "approved"}}}}"#);
        let input: StatusInput = serde_json::from_str(&json).expect("pr-without-number JSON should deserialize");
        let output = render(&input);
        assert!(!output.contains("\u{f407}"));
    }

    // --- effort: ultra (Opus 4.8) ---

    #[test]
    fn statusline_effort_ultra_shows_suffix() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "effort": {"level": "ultra"}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("effort JSON deserializes");
        let output = render(&input);
        assert!(output.contains("Opus·ultra"));
    }

    // --- merge_statusline ---

    fn statusline_value(merged: &str) -> serde_json::Value {
        let root: serde_json::Value = serde_json::from_str(merged).expect("merge output is JSON");
        root["statusLine"].clone()
    }

    #[test]
    fn merge_creates_from_nothing() {
        let merged = merge_statusline(None, "/opt/bin/cc").expect("merge from None");
        let entry = statusline_value(&merged);
        assert_eq!(entry["type"], "command");
        assert_eq!(entry["command"], "/opt/bin/cc");
        assert_eq!(entry["refreshInterval"], 10);
        assert!(merged.ends_with('\n'));
    }

    #[test]
    fn merge_treats_empty_file_as_new() {
        let merged = merge_statusline(Some("  \n"), "/opt/bin/cc").expect("merge from blank");
        assert_eq!(statusline_value(&merged)["command"], "/opt/bin/cc");
    }

    #[test]
    fn merge_preserves_sibling_keys() {
        let existing = r#"{"model": "opus", "env": {"FOO": "1"}}"#;
        let merged = merge_statusline(Some(existing), "/opt/bin/cc").expect("merge");
        let root: serde_json::Value = serde_json::from_str(&merged).expect("output is JSON");
        assert_eq!(root["model"], "opus");
        assert_eq!(root["env"]["FOO"], "1");
        assert_eq!(root["statusLine"]["command"], "/opt/bin/cc");
    }

    #[test]
    fn merge_replaces_statusline_wholesale() {
        // Stale sub-keys must not survive the merge (jq assignment semantics).
        let existing = r#"{"statusLine": {"type": "command", "command": "/old", "padding": 2}}"#;
        let merged = merge_statusline(Some(existing), "/new").expect("merge");
        let entry = statusline_value(&merged);
        assert_eq!(entry["command"], "/new");
        assert!(entry.get("padding").is_none());
    }

    #[test]
    fn merge_rejects_invalid_json() {
        assert!(merge_statusline(Some("{not json"), "/bin/cc").is_err());
    }

    #[test]
    fn merge_rejects_non_object_top_level() {
        assert!(merge_statusline(Some("[1, 2, 3]"), "/bin/cc").is_err());
    }

    #[test]
    fn merge_escapes_windows_paths() {
        let merged =
            merge_statusline(None, r"C:\Users\ceej\.claude\cc-statusline-rs.exe").expect("merge with backslash path");
        assert_eq!(
            statusline_value(&merged)["command"],
            r"C:\Users\ceej\.claude\cc-statusline-rs.exe"
        );
    }
}
