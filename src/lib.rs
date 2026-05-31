use serde::Deserialize;
use std::io::{self, Read};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
// `Default`, so partial/empty JSON deserializes gracefully without per-field attrs.
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

pub fn statusline() -> String {
    let input = read_input().unwrap_or_default();
    render(&input)
}

fn render(input: &StatusInput) -> String {
    let model_display = if let Some(ref model) = input.model.display_name {
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
    } else {
        String::new()
    };

    let context_display = if let Some(ref ctx) = input.context_window {
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
        let tick_at = (ctx.context_window_size > 200_000).then(|| {
            ((200_000.0 * bar_width as f64) / ctx.context_window_size as f64).round() as usize
        });
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
    } else {
        String::new()
    };

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

    let rate_limits_display = input
        .rate_limits
        .as_ref()
        .map(|rl| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
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

    let mut components = Vec::new();
    if !model_display.is_empty() {
        components.push(model_display);
    }
    if !context_display.is_empty() {
        components.push(context_display);
    }
    if !cost_display.is_empty() {
        components.push(cost_display);
    }
    if !agent_display.is_empty() {
        components.push(agent_display);
    }
    if !duration_display.is_empty() {
        components.push(duration_display);
    }
    if !rate_limits_display.is_empty() {
        components.push(rate_limits_display);
    }

    let components_str = if components.is_empty() {
        String::new()
    } else {
        format!(
            " {GRAY}• {RESET}{}",
            components.join(&format!(" {GRAY}• {RESET}"))
        )
    };

    match branch_display {
        Some(branch) => format!(
            "{vim_display}{CYAN}{display_dir}{RESET} {LIGHT_BLUE}\u{f02a2}{RESET} {branch}{lines_changed}{components_str}"
        ),
        None => format!("{vim_display}{CYAN}{display_dir}{RESET}{components_str}"),
    }
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
                status.ahead = a
                    .strip_prefix('+')
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            }
            if let Some(b) = parts.next() {
                status.behind = b
                    .strip_prefix('-')
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            status.dirty = true;
        }
    }
    status
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("{:.3}", cost)
    } else {
        format!("{:.2}", cost)
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

fn fish_shorten_path(path: &str) -> String {
    let home = home_dir();
    let path = path
        .strip_prefix(&home)
        .filter(|_| !home.is_empty() && home != "/")
        .map(|rest| format!("~{rest}"))
        .unwrap_or_else(|| path.to_string());

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 1 {
        return path;
    }

    let shortened: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i == parts.len() - 1 || part.is_empty() || *part == "~" {
                part.to_string()
            } else if part.starts_with('.') && part.len() > 1 {
                format!(".{}", part.chars().nth(1).unwrap_or_default())
            } else {
                part.chars()
                    .next()
                    .map(|c| c.to_string())
                    .unwrap_or_default()
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
        assert_eq!(
            fish_shorten_path("/some/deep/nested/directory"),
            "/s/d/n/directory"
        );
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
        let input: StatusInput =
            serde_json::from_str(json).expect("partial JSON should deserialize");
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
        assert_eq!(
            input.cost.as_ref().expect("cost present").total_cost_usd,
            3.50
        );
        assert_eq!(
            input.cost.as_ref().expect("cost present").total_duration_ms,
            Some(120000)
        );
        assert_eq!(
            input
                .worktree
                .as_ref()
                .expect("worktree present")
                .name
                .as_deref(),
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
        let input: StatusInput =
            serde_json::from_str(json).expect("JSON with unknown fields should deserialize");
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

    fn build_statusline_from(input: &StatusInput) -> String {
        render(input)
    }

    #[test]
    fn statusline_missing_workspace() {
        let input = StatusInput::default();
        let output = build_statusline_from(&input);
        assert!(output.contains("missing workspace.current_dir"));
    }

    #[test]
    fn statusline_non_git_dir() {
        let input: StatusInput = serde_json::from_str(r#"{"workspace": {"current_dir": "/tmp"}}"#)
            .expect("non-git dir JSON should deserialize");
        let output = build_statusline_from(&input);
        assert!(output.contains("/tmp"));
        // No branch indicator for non-git dirs
        assert!(!output.contains("\u{f02a2}"));
    }

    #[test]
    fn statusline_with_model() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "model": {"display_name": "Opus"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("model JSON should deserialize");
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
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
        let input: StatusInput =
            serde_json::from_str(&json).expect("git repo JSON should deserialize");
        let output = build_statusline_from(&input);
        assert!(output.contains("+10"));
        assert!(output.contains("-5"));
    }

    #[test]
    fn statusline_with_agent() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "agent": {"name": "code-reviewer"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("agent JSON should deserialize");
        let output = build_statusline_from(&input);
        assert!(output.contains("code-reviewer"));
    }

    #[test]
    fn statusline_context_color_red() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {"context_window_size": 200000, "used_percentage": 95.0}
        }"#;
        let input: StatusInput =
            serde_json::from_str(json).expect("high context JSON should deserialize");
        let output = build_statusline_from(&input);
        assert!(output.contains("95%"));
        assert!(output.contains(RED));
    }

    #[test]
    fn statusline_context_color_gray() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {"context_window_size": 200000, "used_percentage": 20.0}
        }"#;
        let input: StatusInput =
            serde_json::from_str(json).expect("low context JSON should deserialize");
        let output = build_statusline_from(&input);
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
        let input: StatusInput =
            serde_json::from_str(json).expect("fallback context JSON should deserialize");
        let output = build_statusline_from(&input);
        // (30000+10000+10000)/100000 = 50%
        assert!(output.contains("50%"));
    }

    #[test]
    fn statusline_zero_cost_hidden() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "cost": {"total_cost_usd": 0.0}
        }"#;
        let input: StatusInput =
            serde_json::from_str(json).expect("zero cost JSON should deserialize");
        let output = build_statusline_from(&input);
        // The dollar sign icon should not appear for zero cost
        assert!(!output.contains("\u{f155}"));
    }

    #[test]
    fn statusline_empty_agent_hidden() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "agent": {"name": ""}}"#;
        let input: StatusInput =
            serde_json::from_str(json).expect("empty agent JSON should deserialize");
        let output = build_statusline_from(&input);
        // Agent icon should not appear for empty name
        assert!(!output.contains("\u{f06a9}"));
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
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
        assert!(output.contains("✻"));
    }

    #[test]
    fn statusline_thinking_glyph_hidden_when_disabled() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus"},
            "thinking": {"enabled": false}
        }"#;
        let input: StatusInput =
            serde_json::from_str(json).expect("thinking-disabled JSON deserializes");
        let output = build_statusline_from(&input);
        assert!(!output.contains("✻"));
    }

    #[test]
    fn statusline_strips_one_million_context_suffix() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "model": {"display_name": "Opus 4.7 (1M context)"}
        }"#;
        let input: StatusInput =
            serde_json::from_str(json).expect("model-with-suffix JSON deserializes");
        let output = build_statusline_from(&input);
        assert!(output.contains("Opus 4.7"));
        assert!(!output.contains("(1M context)"));
    }

    // --- vim mode ---

    #[test]
    fn statusline_vim_normal_renders_n() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "vim": {"mode": "NORMAL"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("vim JSON deserializes");
        let output = build_statusline_from(&input);
        assert!(output.contains("[N]"));
    }

    #[test]
    fn statusline_vim_insert_renders_i() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}, "vim": {"mode": "INSERT"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("vim JSON deserializes");
        let output = build_statusline_from(&input);
        assert!(output.contains("[I]"));
    }

    #[test]
    fn statusline_vim_absent_renders_nothing() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}}"#;
        let input: StatusInput = serde_json::from_str(json).expect("no-vim JSON deserializes");
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
        assert!(output.contains("5h"));
        assert!(output.contains("79%"));
        assert!(output.contains("7d"));
        assert!(output.contains("34%"));
    }

    fn epoch_now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
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
        let input: StatusInput =
            serde_json::from_str(&json).expect("rate-limits JSON deserializes");
        let output = build_statusline_from(&input);
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
        let input: StatusInput =
            serde_json::from_str(&json).expect("rate-limits JSON deserializes");
        let output = build_statusline_from(&input);
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
        let input: StatusInput =
            serde_json::from_str(&json).expect("rate-limits JSON deserializes");
        let output = build_statusline_from(&input);
        assert!(output.contains("88%"));
        // The countdown renders as `<pct>%{RESET}{GRAY}·<dur>`; that signature must
        // be absent when the window has already reset.
        assert!(!output.contains(&format!("%{RESET}{GRAY}\u{b7}")));
    }

    #[test]
    fn statusline_rate_limits_absent_renders_nothing() {
        let json = r#"{"workspace": {"current_dir": "/tmp"}}"#;
        let input: StatusInput =
            serde_json::from_str(json).expect("no-rate-limits JSON deserializes");
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
        assert!(output.contains('┊'));
    }

    #[test]
    fn statusline_no_tick_on_200k_window() {
        let json = r#"{
            "workspace": {"current_dir": "/tmp"},
            "context_window": {"context_window_size": 200000, "used_percentage": 50}
        }"#;
        let input: StatusInput = serde_json::from_str(json).expect("200k-window JSON deserializes");
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
        assert!(output.contains("#1234"));
        let idx = output
            .find("\u{f407}")
            .expect("output should contain pr glyph");
        assert!(output[..idx].ends_with(GREEN));
    }

    #[test]
    fn statusline_pr_badge_changes_requested_red() {
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(
            r#"{{"workspace": {{"current_dir": "{this_dir}"}}, "pr": {{"number": 7, "review_state": "changes_requested"}}}}"#
        );
        let input: StatusInput = serde_json::from_str(&json).expect("pr JSON should deserialize");
        let output = build_statusline_from(&input);
        assert!(output.contains("#7"));
        let idx = output
            .find("\u{f407}")
            .expect("output should contain pr glyph");
        assert!(output[..idx].ends_with(RED));
    }

    #[test]
    fn statusline_pr_badge_absent_renders_nothing() {
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(r#"{{"workspace": {{"current_dir": "{this_dir}"}}}}"#);
        let input: StatusInput =
            serde_json::from_str(&json).expect("no-pr JSON should deserialize");
        let output = build_statusline_from(&input);
        assert!(!output.contains("\u{f407}"));
    }

    #[test]
    fn statusline_pr_badge_no_number_hidden() {
        let this_dir = env!("CARGO_MANIFEST_DIR");
        let json = format!(
            r#"{{"workspace": {{"current_dir": "{this_dir}"}}, "pr": {{"review_state": "approved"}}}}"#
        );
        let input: StatusInput =
            serde_json::from_str(&json).expect("pr-without-number JSON should deserialize");
        let output = build_statusline_from(&input);
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
        let output = build_statusline_from(&input);
        assert!(output.contains("Opus·ultra"));
    }
}
