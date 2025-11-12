# Rust Rewrite Plan

This document tracks the feature surface of the legacy Python bot so we can ensure every command is accounted for during the Rust rewrite. The lists below were produced by inspecting `python discord bot/cogs/*` and will drive the Discord-layer design described in `AGENTS.md`.

## Prefix Commands (current bot)

### Music & Audio
- `!dashboard` / `!muziek` (admin) – regenerate the “now playing” dashboard embeds and restart the auto-refresh loop.
- `!join` – force the bot to connect to the caller’s voice channel and bootstrap the dashboard if missing.
- `!leave` – clear the queue, tear down the dashboard, disconnect from voice, and delete cached downloads.
- `!p <search>` – primary music command; connects if needed, resolves YouTube/Spotify queries or playlists, queues tracks, and starts playback.
- `!s [count]` – skip the current track or the specified number of queued tracks (cap of 50).
- `!w` – render the current queue (up to 50 entries) plus total duration.
- `!stop` – pause playback (used as a soft stop/resume toggle).
- `!lyrics` – fetch lyrics for the track that is currently playing through the lyrics manager.
- `!voicestatus` – diagnostics embed summarizing connection state, latency, and queue depth.
- `!nu` – “now playing” snapshot showing the active song and remaining runtime.

### Leveling, XP & Achievements
- `!profile` – per-user level card with progress bar, XP totals, lifetime messages, and daily streak.
- `!xpstats [member]` – detailed analytics embed for the specified member (defaults to the caller).
- `!leaderboard` – paginated leaderboard showing top community members (button-driven within the cog).
- `!daily` – grant the daily XP reward, streak bonus, and server goal progress.
- `!achievements [member]` – list earned achievements grouped by category plus reward totals.
- `!nextachievement` / `!nextach` – estimate the closest unearned achievement and show progress toward it.

### Operations, Admin & Automation
- `!devping ...` (bot admin only) – command group powering developer check-ins:
  - `status`, `setchannel`, `addrole`, `removerole`, `setinterval`, `setdeadline`, `mode`, `pardon`, `unpardon`, `pardons`, `start`.
- `!logging ...` (server admin) – activity logging group with `setchannel`, `enable`, and `disable`.
- `!serverstats ...` (server admin) – guild stats setup utilities with `setup`, `remove`, and `status`.
- `!statstest` – quick sanity check that the server stats cog is responsive.
- `!github ...` (Manage Server) – GitHub tracking group:
  - `track`, `track_org`, `remove`, `remove_org`, `list`, `check`.

### Utilities, Info & AI
- `!info` – Greybeard onboarding embed (also triggered automatically by the AI cog).
- `!help` – categorized help menu that merges prefix and slash metadata.
- `!timezones` / `!tz` / `!times` – snapshot of core team time zones.
- `!deepseek <query>` – invoke the OpenRouter/DeepSeek integration with optional reasoning output.

## Slash Commands (current bot)

Many slash commands are thin shims around the prefix commands above; others call directly into their cogs for guild-management workflows. All of them live in `cogs/slash_commands.py` unless noted otherwise.

### Music & Progression
- `/play <search>` – bridges to `!p` so slash users can queue music.
- `/hello` – health-check ping.
- `/profile`, `/achievements [member]`, `/nextachievement` – bridge to the leveling commands for easier discovery.

### Server Stats
- `/serverstats_setup`, `/serverstats_status`, `/serverstats_remove` – admin-only shims that proxy to the `!serverstats` subcommands.
- `/stats_setup`, `/stats_status` (defined inside `cogs/server_stats.py`) – older slash helpers that directly call the same cog methods; decide whether we keep both name families in Rust.

### Activity Logging
- `/logging_setchannel`, `/logging_enable`, `/logging_disable`, `/logging_status` – configure the Carl-bot-style logging cog from slash UI.


### Information
- `/info` – serves the Greybeard onboarding embed (defined in `cogs/info.py`).

## Next Steps For The Rewrite

1. Map each command (prefix and slash) to its Clean Architecture layer responsibilities so we know what belongs in `core/`, `infra/`, and the Rust Discord adapters.
2. Decide which commands should remain as both prefix and slash versus consolidating purely around slash interactions in the Rust implementation.
3. For admin tooling (dev pings, logging, server stats, GitHub), capture the persistence needs and background jobs so we can spec out the corresponding services and infra traits before coding.
