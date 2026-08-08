# cc-statusline-rs

A fast Rust statusline for Claude Code. It reads the status-hook JSON on stdin and prints one ANSI-colored line.

Idea from [steipete's gist](https://gist.github.com/steipete/8396e512171d31e934f0013e5651691e).

<img width="1524" height="196" alt="image" src="https://github.com/user-attachments/assets/d225dfc1-c4ef-4288-8c94-c7e3fbd16ade" />

## What it shows

- Working directory, fish-style shortened
- Git branch, with dirty, ahead/behind, and worktree markers, plus a PR badge colored by review state
- Lines added and removed this session
- Model name, with reasoning-effort suffix, thinking glyph, and output style
- Context-window usage as a progress bar, with a tick at the 200k boundary on 1M-context models
- Session cost and duration
- Agent name, when running as a sub-agent
- Rate-limit windows (5h and 7d) with reset countdowns, for subscribers
- A prompt-cache expiry countdown, so you know when your next message reheats a cold cache

Components with no data vanish from the line.

## Requirements

- A [Nerd Font](https://www.nerdfonts.com/) in your terminal. The line is full of Nerd Font glyphs; without one, it's a row of boxes.
- [Rust](https://rustup.rs/)
- [just](https://just.systems/) (`brew install just`)
- `jq` (the install recipe uses it)
- `git` on your PATH

## Install (macOS)

```bash
git clone https://github.com/ceejbot/cc-statusline-rs && cd cc-statusline-rs && just install
```

`just install` builds a release binary, copies it to `~/.claude/cc-statusline-rs`, ad-hoc codesigns it, and rewrites the `statusLine` entry in `~/.claude/settings.json` with `refreshInterval: 10`. Fair warning: it edits that file in place.

## Install (Linux and everywhere else)

The `just install` recipe is macOS-only (it runs `xattr` and `codesign`). Do it by hand instead:

```bash
cargo build --release
cp target/release/cc-statusline-rs ~/.claude/
```

Then add this to `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.claude/cc-statusline-rs",
    "refreshInterval": 10
  }
}
```

The 10-second refresh is what lets the cache-expiry countdown tick while the session is idle.

## License

[Apache-2.0](./LICENSE).
