<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo-dark.png">
    <img alt="ai-memory" src="docs/logo-light.png" width="480">
  </picture>
</p>

> Long-term memory for AI coding agents. Quit Claude Code mid-task,
> start OpenAI Codex in the same directory, continue without
> re-explaining the architecture, the failed approaches, or the open
> questions.

[![Release](https://img.shields.io/github/v/release/Aerox912/ai-memory)](https://github.com/Aerox912/ai-memory/releases/latest)
[![Rust](https://img.shields.io/badge/rust-1.95+-blue)](rust-toolchain.toml)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

## Why ai-memory

Your coding agent already has a memory feature. Claude Code takes its own
notes, Cursor remembers some things, and every platform is adding more. All
of them share the same walls: the notes live on one machine, belong to one
agent, and vanish from view the moment you switch tools — or teammates.

ai-memory is what's on the other side of those walls.

- **It follows you across agents.** Twenty-plus harnesses — Claude Code,
  Codex, Cursor, Gemini CLI, OpenCode, Grok, Devin, Kimi, Kiro, and more —
  feed one shared memory. Quit Claude Code mid-task, open Codex in the same
  directory, and the next agent picks up a real handoff: where you left
  off, what failed, what's still open. Handoffs are a protocol here, not a
  convention — typed, owned, claimed exactly once.

- **It follows you across machines.** Memory lives in a server you run —
  on the same laptop, a homelab box, or wherever — so the project you left
  on the desktop is the project you resume on the laptop. Same knowledge,
  same open questions.

- **It works for a team.** Point everyone at one server and what one
  person's sessions learn, everyone's agents can retrieve. Knowledge is
  shared per project; personal handoffs stay personal. Multi-user auth,
  per-person attribution, and an audit log are built in — not a paid tier.

- **Your memory is plain markdown.** The source of truth is a git-backed
  wiki of ordinary `.md` files: `grep` it, open it in Obsidian, edit it by
  hand, `rsync` it. The database is a derived index that can always be
  rebuilt from the files. No vector store to babysit, nothing held hostage
  in a binary blob.

- **It captures the work itself, silently.** Lifecycle hooks record what
  actually happened — prompts, tool calls, session boundaries — sanitized
  at a typed privacy boundary before anything is stored, then consolidated
  into readable pages. No "remember this" ceremony. And the default path
  uses **zero LLM calls**: capture, search, and handoffs all work with no
  API key at all.

- **It tells you the truth about itself.** One self-contained binary.
  Purge commands that say exactly what "deleted" means. A measured write
  ceiling (~700/s) instead of a guessed one. An audit log of every
  mutation. Boring, in the way infrastructure should be.

## How it works

```
capture ──▶ consolidate ──▶ recall ──▶ handoff
 hooks        session-end      search     next agent,
 observe      summaries as     + brief    any harness
 silently     wiki pages       injection
```
| Area | Status | Notes |
|---|---|---|
| Linux | Supported | The Aerox912 fork publishes the native `ai-memory-linux-x86_64.tar.gz` bundle used by WSL. Upstream remains the source for ARM Linux, Docker, and AUR artifacts. |
| macOS | Supported upstream | Source builds and upstream releases remain available, but this workstation fork does not publish macOS artifacts. See [`docs/macos.md`](docs/macos.md). |
| Windows via WSL2 | Supported | Use the Linux install path inside WSL2 when the agent runs there. |
| Native Windows | Experimental | Tagged releases publish `ai-memory-windows-x86_64.zip` with `ai-memory.exe`; Docker Desktop wrapper and source builds are also available. Local supported profiles default to host-native hook commands; Claude Code may use its Windows exec form, while other agents use native single command strings matching their hook schema. PowerShell/Git Bash scripts are compatibility fallbacks. See [`docs/windows.md`](docs/windows.md). |
| Claude Code | Supported | MCP config + lifecycle hooks; native commands enforce capture exclusions. `install-mcp --session-aware` optionally enables per-session auto-scope isolation through a local stdio bridge. Optionally captures the assistant's final turn on `Stop` when installed with `--capture-assistant` and the server enables `capture_assistant` (double opt-in, off by default). |
| Codex | Supported | MCP config + lifecycle hooks; native commands enforce capture exclusions. No automatic true session-end hook, so run `ai-memory finalize-session` when you need a final summary/handoff. |
| Command Code | Supported | MCP config (`~/.commandcode/mcp.json`) + its four stable lifecycle-hook events (`~/.commandcode/settings.json`); native commands enforce capture exclusions and `SessionStart` injects handoffs. `Stop` is only a turn boundary, so use `ai-memory finalize-session --agent command-code` after the final turn. `ai-memory run command-code` adds exact v3 native-session resume and visible-event import; experimental unsandboxed Mods remain excluded. |
| Devin CLI | Supported | MCP config + lifecycle hooks. Hooks use Devin's `PostCompaction` event, inject handoffs via `hookSpecificOutput.additionalContext`, and omit subagent events because Devin does not expose them. |
| OpenCode | Supported | Remote MCP config + generated TypeScript plugin; generated plugin enforces capture exclusions. |
| Cursor | Supported | MCP config + lifecycle hooks. |
| Gemini CLI | Supported | MCP config + lifecycle hooks. |
| Oh My Pi / OMP | Supported | Use `--client omp` / `--agent omp` (or `oh-my-pi`) for native `.omp` MCP config + TypeScript extension; generated extension enforces capture exclusions. |
| Pi | Supported | Generated `~/.pi/agent/extensions/ai-memory-pi.ts` extension provides lifecycle capture and an HTTP MCP bridge; generated extension enforces capture exclusions. |
| Crush | Managed-only | `ai-memory run crush` resumes its project-local session database and supplies portable context through a temporary supported global-context file; no lifecycle-hook installer is provided. |
| Managed workstreams | Opt-in | `ai-memory run` provides transparent cross-harness continuity for Claude Code, Codex, OpenCode, Pi, Crush, Kimi Code, Command Code, both incompatible Kiro CLI engines, OMP, Grok Build CLI, and Antigravity CLI. Direct launches remain unchanged. See [`docs/managed-workstreams.md`](docs/managed-workstreams.md). |
| Claude Desktop | MCP-only | Uses `mcp-remote`; no lifecycle hooks. |
| OpenClaw | Supported | MCP config + native plugin lifecycle hooks; generated plugin enforces capture exclusions. |
| Antigravity CLI | Supported | MCP config (`serverUrl`) + lifecycle hooks (`agy` alias). Only `PreInvocation` with `invocationNum = 0` maps to SessionStart; later model calls cannot consume a next-session handoff. No automatic true session-end hook, so run `ai-memory finalize-session --agent antigravity-cli` after the final turn when you need a summary, handoff, and opt-in SessionEnd consolidation. `ai-memory run antigravity` (aliases `antigravity-cli`, `agy`) adds managed workstream resume via `--conversation`; conversation text is not decoded, so the ledger for this harness comes from hook capture. |
| Grok Build CLI | Supported | MCP config (`install-mcp --client grok` → `$GROK_HOME/config.toml`, default `~/.grok/config.toml`) + lifecycle hooks (`install-hooks --agent grok` → `$GROK_HOME/hooks/ai-memory.json`, default `~/.grok/hooks/ai-memory.json`, Grok-specific hook bundle). Capture works; no hook handoff injection — Grok ignores `SessionStart` stdout, so recover handoffs via MCP `memory_handoff_accept`. `ai-memory run grok` adds managed workstream resume with the context packet delivered natively through `--rules`. Skills root: `.grok/skills` / `$GROK_HOME/skills` (default `~/.grok/skills`). |
| Swival CLI | MCP-only | `install-mcp --client swival --apply` merges a native HTTP entry into the project-root `.swival/mcp.json`, preserving sibling servers. Lifecycle and managed-workstream support are not claimed because Swival's callback contract does not expose a stable session identifier. |
| Zero | Supported | `install-mcp --client zero` (native HTTP + bearer in `~/.config/zero/config.json`) + lifecycle hooks via `install-hooks --agent zero --apply` (exec-form native commands in `~/.config/zero/hooks.json`, JSON payload on stdin, no shell). Capture works incl. specialist (subagent) events; no handoff injection — Zero discards `sessionStart` stdout, so recover handoffs via MCP `memory_handoff_accept`. |
| Kimi Code | Supported | MCP config (`url` entry in `~/.kimi-code/mcp.json`) + lifecycle hooks (`[[hooks]]` in `~/.kimi-code/config.toml`, 10 events including subagent start/stop and `PostToolUseFailure` for tool-failure capture); both paths honor `$KIMI_CODE_HOME`. Handoffs inject via `UserPromptSubmit` stdout (Kimi Code discards `SessionStart` hook stdout); `ai-memory run kimi` adds managed workstream resume. |
| Kiro CLI | Supported | MCP config uses `install-mcp --client kiro-cli` (alias `kiro`) and Kiro's Bedrock-compatible schema flavor. `install-hooks --agent kiro-cli` merges v2 hooks into existing agent configs; the explicit `--agent kiro-cli-v3` target writes the incompatible standalone v3 registration. Both preserve unrelated entries, honor `$KIRO_HOME`, enforce capture exclusions, and inject pending handoffs at session start. Kiro has no true SessionEnd hook; use `ai-memory finalize-session --agent kiro-cli`, with `--session-id <uuid>` for concurrent sessions. `ai-memory run kiro` manages v2; add `--v3`, `--mode`, or `--agent-engine v3` for version-safe v3 resume. |
| Pool | Hooks-only | Poolside Agent CLI (`pool`). Lifecycle-hook capture via `install-hooks --agent pool` (alias `poolside`): Pool reads project-scoped hooks from the repo-root `.poolside/settings.yaml`, so ai-memory stages the scripts and prints a ready-to-paste `hooks:` snippet rather than writing project-local files; native commands enforce capture exclusions. Five Claude-shaped events (`SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Stop`) and no true session-end — `Stop` is a turn boundary, so run `ai-memory finalize-session --agent pool` after the final turn. `SessionStart` stdout injection is not demonstrated, so capture works but handoff injection does not — recover handoffs via MCP `memory_handoff_accept`. No first-party `install-mcp` client and no managed workstream (`ai-memory run pool`) are claimed: Pool's native session-store contract is not demonstrated (see [`docs/managed-harness-contributions.md`](docs/managed-harness-contributions.md)). Verified against Poolside CLI v1.0.16. |
| VS Code Copilot | MCP-only | `.vscode/mcp.json` for Copilot agent mode; no lifecycle hooks (Copilot does not expose them yet). |
| Zed | MCP-only | Native remote MCP under `context_servers` in Zed's user `settings.json`; no lifecycle hooks or managed-workstream support. |
| Hermes Agent | Community | Core hook ingestion recognizes `agent=hermes` and Hermes' documented shell-hook `tool_name` / `tool_input` payload for concrete session attribution, tool-family titles, and capture exclusions. A community-maintained [`ai-memory-hermes-plugin`](https://github.com/MrLuciano/ai-memory-hermes-plugin) is available, but no first-party installer is shipped; review its compatibility matrix, install/uninstall scripts, and secret handling before using it. Hermes ignores session-start hook stdout, so recover handoffs through MCP. |
| LLM/auth providers | Supported | Anthropic, OpenAI, OpenAI OAuth/Codex, GitHub Copilot, Gemini, OpenCode Zen/Go, OpenAI-compatible endpoints, and generic OIDC device auth for native hooks. |
| Embedding providers | Supported | OpenAI, Voyage, Google Gemini, and keyless OpenAI-compatible endpoints such as Ollama, LM Studio, and vLLM. |

## What it is

LLM coding agents lose context when a session ends. ai-memory gives them a
shared, persistent wiki compiled from sanitized lifecycle observations. When a
session ends, relevant observations become a coherent summary; the next agent
receives a bounded handoff. Optional `ai-memory run` launches add a portable
visible-event ledger and native per-harness resume for higher-fidelity
cross-harness continuity.

The wiki is plain markdown in a git repo - `grep`-able, openable in
Obsidian, backed up with `rsync`. No vector database to babysit, no
`write_note` ceremony, no manual context-loading. The full design is
in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md); the influences and
priors are at the [bottom](#influences-and-prior-art).

## Key features

- **Zero-friction lifecycle capture.** Hooks fire-and-forget bounded,
  sanitized prompt, tool-lifecycle, and session-boundary observations. Direct
  launches keep this lightweight path; it is not a complete native transcript.
  User prompts and post-compaction summaries retain up to 16 KiB;
  notifications and tool excerpts retain up to 2 KB, with a 16 KiB durable
  backstop for every observation body.
- **Opt-in managed workstreams.** `ai-memory run claude`, then `ai-memory run
  codex --yolo`, then `ai-memory run command-code`, transparently resumes one
  logical workstream with native per-harness sessions, a portable visible-event
  ledger, and full-ledger search. Delivered packets are origin-marked; Claude
  transcript import rejects a packet that Claude persisted and read back through a tool.
  `ai-memory run` with no harness continues the newest usable Claude Code,
  Codex, OpenCode, Pi, Crush, Kimi Code, Command Code, or Kiro CLI v2/v3
  session for this checkout.
  On first
  explicit use, an interactive launcher can adopt a previous session from the
  same checkout; later switches cannot select unrelated native history. Native
  arguments pass through unchanged except the wrapper-owned `--yolo` and
  `--fresh`; direct
  commands are unaffected. `kimi-code` and `kimi-cli` are accepted aliases for
  the installed `kimi` command; `commandcode`, `cmdc`, and `cmd` select the
  cross-platform `command-code` executable (`cmdc` on native Windows); and
  `kiro-cli` selects the installed `kiro-cli` command. Kiro defaults to v2;
  `ai-memory run kiro --v3` selects v3, while a
  returning linked v3 workstream selects its engine transparently.
- **Per-repository capture exclusions.** A nearest-marker `[capture]`
  `ignore_paths` policy drops matching recognized file-tool events before they
  reach the local spool or server. See [the capture policy reference](docs/marker-file.md#capture-exclusions).
- **Opt-in capture scope.** `install-hooks --capture-mode allowlist` inverts
  the default so a repository without a marker emits no lifecycle event at
  all, dropped by the native hook before the spool. Forgetting a marker then
  costs recall rather than confidentiality. Enforced by native `ai-memory
  hook` commands only — see [allowlist mode](docs/marker-file.md#allowlist-mode-the-marker-as-an-opt-in).
- **Optional per-operator memory slots.** On shared servers,
  `[slots] per_user = true` keeps engine-written `_slots/` context in a bounded
  namespace derived from the authenticated operator. Session briefs and
  consolidation prompts receive shared slots plus the caller's own; exact wiki
  reads and searches remain project-wide, so this is context-injection
  isolation rather than RBAC. See [multi-user operation](docs/users.md#per-operator-memory-slots).
- **Cross-agent handoffs.** Quit Claude Code mid-task, start Codex
  in the same directory hours later - the next agent sees a
  "where you left off" block before its first prompt.
- **Per-project isolation by construction.** Each project lives at
  `<wiki_root>/<workspace_id>/<project_id>/…` keyed by stable UUIDs.
  Workspace defaults to `"default"`. Project is derived from `$cwd`:
  CLI subcommands (`bootstrap`, `write-page`, `lint`, …) walk to the
  main git repo root so all worktrees of the same repo share one
  project identity; the hook router defaults to `basename($cwd)` and
  can opt into the repo-root rule. Drop a
  [`.ai-memory.toml` marker file](docs/marker-file.md) in any
  ancestor directory to override either field explicitly — perfect for
  multi-client consultancies, work/personal split, mono-repos, or
  linked git worktrees.
  Same page path can exist in two projects without collision; a
  rename is one column update; a purge is one `rm -rf`.
- **Global preferences scope.** Standing user/team context — tech
  choices, code style, durable personal rules — lives in the reserved
  `_global` scope (`memory_write_page` with `scope: "global"`). Default
  `memory_query` reads union it into every project as
  `global_scope_hits`, so preferences travel with you into new projects
  without naming a magic project or paying the all-projects
  `global=true` fan-out. Event capture never writes there.
- **Entity-assisted recall.** Consolidation stores up to 10 specific nouns per
  page in canonical `entities:` frontmatter. Exact, prefix, and compound-word
  matches form a project-scoped RRF stream, so a query can recover a page even
  when its body uses different wording. The stream is lexical and adds no
  query-time LLM call.
- **Authority-aware recall.** FTS5, entity-match RRF, graph-neighbor RRF, and
  optional vector RRF generate candidates by relevance. Before truncation, a
  bounded adjustment favors maintained `_rules/`, `decisions/`, `procedures/`, and
  `gotchas/` pages over closely matching episodic session evidence. Tier,
  `pinned`, and explicit `canonical` / `active` / `source-of-truth` or
  `superseded` / `historical` / `test-fixture` / `do-not-answer-from` tags
  contribute without becoming absolute filters, so targeted history searches
  still find session pages. These signals affect retrieval provenance only;
  retrieved text remains untrusted historical evidence and never gains
  instruction authority from its namespace, tier, tags, pin, or rank.
- **Clear routing alongside code-intelligence tools.** Run ai-memory beside a
  structural MCP server, LSP, or other live-code tool without synchronizing
  their stores. Use memory for prior decisions, rationale, failed attempts,
  procedures, and handoffs; use the current checkout and structural provider
  for symbols, callers, dependencies, and impact analysis. Verify historical
  code claims against the checkout before acting, and treat source, builds,
  tests, and observed runtime behavior as operational truth. See
  [Historical memory and live code intelligence](docs/usage.md#historical-memory-and-live-code-intelligence).
- **Karpathy-style LLM wiki.** Pages are compiled from observations
  at session-end (or PreCompact; clients without a true session-end event can
  use `ai-memory finalize-session --agent <agent>` for a manual final close),
  not retrieved over raw logs.
  Supersession chain + git-versioned markdown means you can
  time-travel with `ai-memory checkpoints`, `restore-page`, or raw `git log`.
- **Built-in `/web` browser.** Read-only HTML UI for the wiki -
  project list, folder tree, FTS5 search, markdown rendering, dark
  mode. Mounted on the same axum server as MCP.
- **Server-wide MCP client activity.**
  `GET /admin/activity/by-client?since_days=7` shows which MCP clients are
  calling memory tools, split into reads and writes. Counts use bounded UTC-day
  buckets, so arbitrary client names cannot grow the database with request
  volume; shared deployments keep the endpoint root-only. See
  [MCP client activity](docs/users.md#mcp-client-activity).
- **Multi-agent + multi-machine ready.** Supported clients: Claude
  Code, Codex, Command Code, Devin CLI, OpenCode, Cursor, Claude Desktop (via `mcp-remote`),
  Gemini CLI, Antigravity CLI, Grok Build CLI, Kimi Code, OpenClaw, Oh My Pi
  / OMP (`omp` / `oh-my-pi`), Pi via generated bridge extension, VS Code
  GitHub Copilot agent mode (MCP-only, workspace `.vscode/mcp.json`), Kiro CLI
  (MCP + v2 lifecycle hooks), Pool (hooks-only, project
  `.poolside/settings.yaml` snippet), and Zed (MCP-only, user `settings.json`).
  Server runs local (loopback) OR on a homelab box (LAN/VPN/cloud)
  with bearer-token auth. Shared servers can opt into
  [`[auto_scope]` modes](docs/auto-scope.md) for per-user or
  session-aware current-project routing; Claude Code has a built-in opt-in
  bridge via `install-mcp --session-aware`.
- **Thin-client CLI.** `ai-memory status`, `bootstrap`, `checkpoints`,
  `restore-page`, `purge-project`, `rename-project`, `move-project`,
  `move-session`,
  `audit-contamination`, `lint`, `curator`, `auto-improve`,
  `auto-improve-report`, `pending-writes`, `embed`, `forget-sweep`, `backup`,
  `finalize-session` are
  all HTTP clients of the running server - never touch SQLite or
  wiki files directly. `status` also reports passive LLM/embedding
  provider health from the last real provider call. Server is the
  single source of truth. `finalize-session` lists matching open
  sessions through `GET /admin/open-sessions`, then posts synthetic
  `session-end` hooks back to the server. On shared deployments it defaults to
  the caller's own plus unattributed sessions; root can pass `--all-owners` for
  explicit cross-operator recovery. When concurrent sessions share an agent and
  scope, pass `--session-id <uuid>` to target one exact open session; it cannot
  be combined with `--all`.
- **LLM is opt-in.** Zero-LLM mode still gives you FTS5, manually declared
  entity, and graph-neighbor search plus rule-based summarisation. Add a
  provider when you want consolidated pages, lint contradictions, or staged
  auto-improvement proposals.

## Use cases

- **"Quit Claude Code and continue the same work in Codex."** Use the optional
  managed launcher when you want native session resume plus the portable visible
  history, not only a summary handoff:

  ```bash
  cd /path/to/project
  ai-memory run claude

  # Quit Claude Code, then continue the same workstream in Codex.
  ai-memory run codex --yolo

  # Continue in Command Code, preserving its own exact native session.
  ai-memory run command-code

  # Later, omit the name to resume the newest usable managed session here.
  ai-memory run

  # Start a new Codex session in the same workstream, keeping portable history.
  ai-memory run --fresh codex

  # Kiro defaults to v2; select its incompatible v3 engine explicitly once.
  ai-memory run kiro --v3

  # List the workstreams that can be selected from this checkout.
  ai-memory workstreams

  # List open cross-agent handoffs, oldest first, with the id
  # `memory_handoff_cancel` needs to clear a stale one.
  ai-memory handoffs
  ```

- **"Pick the project instead of remembering where it lives."** Start from a
  directory containing your checkouts and choose the checkout before the
  managed harness:

  ```bash
  ai-memory show

  # Machine-readable discovery without launching anything.
  ai-memory show --json
  ```

  Each successful `ai-memory run` saves a client-local checkout link keyed by
  the configured server plus workspace/project. `show` joins those links with
  the server's public activity and page-count metadata. A fast, bounded depth-1
  scan of the current directory also finds new checkouts carrying a project
  marker (`.git`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`, and
  friends), while skipping dependency and build directories. The server never
  exposes a checkout path, so two client machines can safely use different
  local paths for the same project on a remote homeserver.

  The list always leads with **`+ New project`**: type a name and ai-memory
  validates a portable directory name, stages the new checkout privately, pins
  its workspace and project in `.ai-memory.toml`, and installs the routing block
  and managed Agent Skills for the chosen agent. The final directory appears
  only after every setup step succeeds, then `show` launches from it.

  The harness menu only offers agents actually installed on the host, using the
  same `PATH` lookup `run` enforces at launch.

  `--no-scan` uses only saved links; `--workspace` filters both sources;
  `--yolo`, `--fresh`, and trailing native arguments are forwarded unchanged.
  Non-terminal use must pass `--json`; JSON mode is discovery-only and never
  launches a harness.

  The first explicit run can offer an existing session from this exact checkout
  or start a new one. Switching harnesses starts or resumes the native session
  linked to the shared workstream, so an obsolete local session cannot replace
  newer cross-harness history. After a normal quit, the next launch waits
  briefly if the previous launcher is still finalizing; handled failures release
  the workstream immediately. If a linked native transcript was deleted,
  ai-memory detects the orphan before launch and starts fresh; `--fresh` forces
  that recovery for one harness. Managed mode currently covers Claude Code,
  Codex, OpenCode, Pi, Crush, Kimi Code, Command Code, Kiro CLI v2/v3, OMP,
  Grok Build CLI, and Antigravity CLI; direct harness launches remain unchanged. See
  [Managed cross-harness workstreams](docs/managed-workstreams.md).
- **"Just put me back where I was."** From any directory, with no name to
  type and no list to read:

  ```bash
  ai-memory continue
  ```

  It picks the checkout whose managed launch is most recent, revalidates the
  path and its resolved scope, then continues there exactly as bare
  `ai-memory run` would. A link whose directory moved, was replaced, now
  resolves to a different project, or has a corrupt ordering timestamp is
  reported on stderr and skipped, so a resume never quietly lands in the wrong
  project. `--workspace` narrows the search; `--yolo` and `--fresh` are
  forwarded.
- **"Quit at 4 PM, pick up at 9 AM in a different agent."** The
  classic. SessionStart hook in the next supported hook client prepends a
  typed handoff with open questions, next steps, and a session summary. Grok
  captures lifecycle events but ignores SessionStart stdout, so ask it to call
  `memory_handoff_accept` when resuming from a handoff. Zero has the same
  no-stdout behavior and also must call `memory_handoff_accept`.
- **"What did we decide about X six weeks ago?"** Use `memory_query X` from
  the agent for FTS5 fused with entity matches and linked-page expansion (plus
  vector similarity when an embedder is configured). For a quick terminal-only
  FTS5 lookup, use `ai-memory search X`; that admin command does not run the
  hybrid streams. Pages are
  LLM-consolidated, so the hit is a coherent decision page, not a raw
  chat log. Pass `explain: true` to see why each hit ranked where it
  did in project or explicit-scope retrieval. Cross-project
  `global: true` search uses its separate FTS-only ranker and reports
  that active stream without per-hit RRF details.
- **"Remember this permanently."** When something is worth keeping
  beyond auto-captured session logs - a decision, a convention, a
  gotcha - tell the agent "save a permanent note that we standardised
  on Postgres for X" or "annotate this as a project rule" and it calls
  `memory_write_page` to write a durable, git-versioned wiki page. From
  a terminal it's `ai-memory write-page --path decisions/0007-db.md
  --body $'# Standardised on Postgres\n\n...' --pinned`. `--pinned`
  exempts it from the decay sweep; the H1 on the first line of
  `--body` becomes the page title (omit `--title` — it's still
  accepted, but LLM callers trip over JSON-escaping their way through
  it, see issue #67). Unlike a handoff (single-use) or an
  auto-synthesised session page (rewritten on consolidation), a
  write-page note is yours: it shows up in `memory_query`, renders in
  `/web`, and stays until you change it.
- **"That page you found is out of date."** The agent calls
  `memory_feedback` with the page's path and a signal: `helpful` /
  `not_helpful` tune how strongly retention keeps a sweep-eligible episodic
  page (they move its salience, which scales the decay formula's time term),
  while `stale` / `wrong` floor the salience *and* make any current page
  show up as a `feedback_flagged` finding in the next `memory_lint` report.
  Feedback never deletes anything — it lowers confidence and flags for review —
  and it attaches to the version current when feedback is recorded, so a
  later rewrite clears the flag. Retrieved page text is untrusted and never
  authorizes feedback by itself.
- **"Remember this, but only until the sprint ends."** Pass
  `expires_at` to `memory_write_page` (RFC3339 or `YYYY-MM-DD` = end of
  that day, UTC) — or put `expires_at:` in a page's frontmatter by
  hand. Past the TTL the page disappears from search/recent/briefing
  (pass `include_expired: true` to `memory_query` to still see it) and
  the next forget sweep hard-deletes the file and its rows. A TTL beats
  a pin; `memory_lint` warns about pinned+expiring combos.
- **"This new project has months of history before ai-memory."**
  `cd /path/to/my-project && ai-memory bootstrap` collects
  `git log`, README, `docs/`, module headers, project rules and
  one-shot-summarises them into seed wiki pages. Future sessions
  build on top.
- **"What durable lesson did that session teach?"**
  When an LLM provider is configured, ai-memory runs a background
  auto-improvement scheduler for newly completed sessions in every project. It
  records proposed wiki edits in the pending-writes audit trail, then approves
  them immediately through the normal wiki write path by default. Scheduler ticks
  are non-overlapping: if reviewing all projects takes longer than the interval,
  the next tick is delayed until the current one finishes. Scheduling and
  approval are separate: set `[auto_improve.scheduler] enabled = false` to stop
  automatic review, or set `[auto_improve] require_approval = true` to keep both
  scheduled and manual proposals pending for human review. `ai-memory
  auto-improve --session-id <uuid>` and MCP `memory_auto_improve` remain
  available for manual catch-up or targeted reruns. When its `session_id` is
  omitted, the MCP tool selects the newest completed session without a
  persisted auto-improvement run, so repeated calls advance past short
  preflight-skipped sessions; an explicit ID reruns that session. `ai-memory
  auto-improve-report --workspace <w> --project <p>` returns a read-only
  telemetry report for recent auto-improvement outcomes without staging or
  creating proposals; add `--stage` to create one pending report page for
  audit/approval. On deployments that distinguish operators, pending learning
  proposals are isolated by qualified operator identity, so one person's
  proposal for a page does not block another's; unattributed and single-user
  deployments retain the shared pending queue. See
  [`docs/auto-improve-eval-gates.md`](docs/auto-improve-eval-gates.md) for
  example executable eval scorers.

Agents emit sanitized observations through lifecycle hooks as you work.
At session end, observations become coherent markdown pages in the
project's wiki (optionally LLM-written; useful even without). The next
session — any agent, any machine — gets a bounded brief and can search
everything: full-text, entities, links, and (optionally) vectors, fused
into one ranking. Cross-agent handoffs carry the baton explicitly.

The full design, including the invariants that keep multi-user and
multi-session use safe, is in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Support matrix

Every row below is a first-party integration — MCP registration, lifecycle
hooks, or both — kept honest by CI. The full matrix with per-agent notes and
caveats is in [`docs/support-matrix.md`](docs/support-matrix.md).

| Area | Status |
| --- | --- |
| Linux | Supported |
| macOS | Supported |
| Windows via WSL2 | Supported |
| Native Windows | Experimental |
| Claude Code | Supported |
| Codex | Supported |
| Command Code | Supported |
| Devin CLI | Supported |
| OpenCode | Supported |
| Cursor | Supported |
| Gemini CLI | Supported |
| Oh My Pi / OMP | Supported |
| Pi | Supported |
| Crush | Managed-only |
| Managed workstreams | Opt-in |
| Claude Desktop | MCP-only |
| OpenClaw | Supported |
| Antigravity CLI | Supported |
| Grok Build CLI | Supported |
| Swival CLI | MCP-only |
| Zero | Supported |
| ZCode | Supported |
| Kimi Code | Supported |
| Kiro CLI | Supported |
| Pool | Hooks-only |
| VS Code Copilot | MCP-only |
| Zed | MCP-only |
| Hermes Agent | Community |
| LLM/auth providers | Supported |
| Embedding providers | Supported |

## Quick start

### Arch Linux (AUR)

For native Arch installs, use the AUR packages. They install
`/usr/bin/ai-memory`, packaged hook sources, and both system-level and
user-level systemd units.

```bash
yay -S ai-memory-bin    # prebuilt Linux x86_64/aarch64 binary
yay -S ai-memory        # builds from source
```

Single-user workstation:

```bash
mkdir -p ~/.config/ai-memory ~/.local/share/ai-memory
ai-memory --data-dir ~/.local/share/ai-memory \
  --config ~/.config/ai-memory/config.toml init
systemctl --user enable --now ai-memory.service
ai-memory install-mcp --client claude-code --apply
ai-memory install-hooks --agent claude-code --apply
```

System service installs use `/var/lib/ai-memory` and `/etc/ai-memory/` via the
packaged unit. Full user-service, system-service, auth, and provider setup is in
[`docs/install.md#arch-linux-native-packages-aur`](docs/install.md#arch-linux-native-packages-aur).

### Docker

You need: Docker + an agent CLI from the [Support Matrix](#support-matrix), or
anything else that speaks MCP.

The published Docker image includes `linux/amd64` and `linux/arm64` variants,
so Apple Silicon Macs and ARM64 Linux hosts can pull `akitaonrails/ai-memory`
without `--platform linux/amd64` emulation.

The default quick-start has **no authentication** - the server binds
to loopback only, so on a single-user laptop nothing else can reach
it. Adding a bearer token is a one-line change once you're ready to
expose the server on the LAN; see [Security](#security) below.

```bash
# 1. Install the ai-memory CLI wrapper (a small shell script that
#    runs the binary inside docker with your $HOME mounted). This is
#    the only thing that needs to live on the host filesystem.
mkdir -p ~/.local/bin
wrapper_tmp="$(mktemp -d)"
trap 'rm -rf "$wrapper_tmp"' EXIT
wrapper_base=https://github.com/akitaonrails/ai-memory/releases/latest/download/ai-memory-wrapper
curl -fsSL "$wrapper_base" -o "$wrapper_tmp/ai-memory-wrapper"
curl -fsSL "$wrapper_base.sha256" -o "$wrapper_tmp/ai-memory-wrapper.sha256"
expected="$(awk 'NR == 1 { print $1 }' "$wrapper_tmp/ai-memory-wrapper.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$wrapper_tmp/ai-memory-wrapper" | awk '{ print $1 }')"
else
    actual="$(shasum -a 256 "$wrapper_tmp/ai-memory-wrapper" | awk '{ print $1 }')"
fi
[ -n "$expected" ] && [ "$actual" = "$expected" ] || { echo "wrapper checksum mismatch" >&2; exit 1; }
install -m 0755 "$wrapper_tmp/ai-memory-wrapper" ~/.local/bin/ai-memory
rm -rf "$wrapper_tmp"
trap - EXIT
# Most distros put ~/.local/bin on PATH automatically. If `which
# ai-memory` comes up empty, add this to ~/.bashrc / ~/.zshrc:
#     export PATH="$HOME/.local/bin:$PATH"

# 2. Start the server. `--restart unless-stopped` makes it come back
#    on docker daemon restart and on machine boot (provided your
#    docker service is enabled at boot — `sudo systemctl enable
#    docker` on most distros). Loopback-only bind (`127.0.0.1:49374`)
#    so nothing outside this machine can reach it. Omit the LLM /
#    EMBEDDING lines for zero-LLM mode — FTS5 search still works
#    without any keys.
docker run -d --name ai-memory \
    --restart unless-stopped \
    -p 127.0.0.1:49374:49374 \
    -v ai-memory-data:/data \
    -e AI_MEMORY_LLM_PROVIDER=anthropic \
    -e ANTHROPIC_API_KEY=sk-ant-... \
    -e AI_MEMORY_EMBEDDING_PROVIDER=openai \
    -e OPENAI_API_KEY=sk-... \
    akitaonrails/ai-memory:latest

# 3. Wire your agent CLI in two commands. The wrapper takes care of
#    mounts and each client's config-path detection. Re-run with
#    `--agent codex`, `--agent command-code`, `--agent devin`, `--agent opencode`, `--agent gemini-cli`,
#    `--agent grok`, `--agent kimi-code`, `--agent kiro-cli`, `--agent omp`,
#    `--agent oh-my-pi`, `--client cursor`,
#    `--client gemini-cli`, `--client grok`, `--client kiro-cli`, etc.
#    for additional agents; full list in docs/install.md.
ai-memory install-mcp   --client claude-code --apply
ai-memory install-hooks --agent  claude-code --apply
```

On Linux/macOS, that's it. Start a Claude Code session as usual - every
prompt and tool call now lands in ai-memory, and the next session you
open in this project will see a handoff with where you left off.
On macOS, the native release binary is also supported and recommended when you
do not need Docker; see [`docs/macos.md`](docs/macos.md).

Wiring another agent is the same two commands with a different name —
`--client codex`, `--agent codex`, and so on for every row of the support
matrix. The full per-agent guide, including Windows and remote servers, is
[`docs/install.md`](docs/install.md).

Two agents in the same project at once, or teammates on one server? That
works out of the box: the "current project" pointer is isolated per caller
by default (v1.39+). See [`docs/auto-scope.md`](docs/auto-scope.md) for the
optional session-aware Claude Code bridge and the details.

Managed workstreams are optional and add cross-harness *session* continuity
on top of shared memory:
The same bridge can serve stdio-only Codex or Antigravity clients without a
Claude session id. Deployment wrappers that already resolved a repository can
run it with `--require-scope-pin --workspace <name> --project <name>` so every
memory call is constrained to that exact scope. Add `--tool-profile recall`
for read-only clients or `--tool-profile session` for recall plus scoped page
writes, feedback, and handoffs. Both discovery and direct calls are filtered;
maintenance, deletion, self-routing, and automatic-improvement tools stay
hidden in these restricted profiles. `full` preserves the upstream surface.

Keep bearer tokens out of tracked MCP configuration. Generic installs can use
`AI_MEMORY_AUTH_TOKEN`; process supervisors and secret-store wrappers can use
`AI_MEMORY_AUTH_TOKEN_FILE` with a file or pipe path instead. Servers can also
load the multi-user token pepper from `AI_MEMORY_TOKEN_PEPPER_FILE`. Direct and
file-backed bearer inputs together are rejected as ambiguous.

The `install-mcp` / `install-hooks` commands use
`AI_MEMORY_SERVER_URL` / `AI_MEMORY_AUTH_TOKEN` when set; otherwise
they default to `http://127.0.0.1:49374` (matching the server above)
and no bearer token. If hooks are installed after an ai-memory MCP
entry already exists, `install-hooks` reuses that endpoint so a remote
MCP setup cannot silently regenerate loopback-only hooks. Both commands
are idempotent - re-runs replace ai-memory's entry, preserve every
other server / hook you have configured, and write a timestamped
`.bak-<ts>` next to the file before each modifying write. The hook
scripts are staged into `~/.local/share/ai-memory/hooks/<agent>/`
automatically; re-running overwrites them so future image updates ship
updated hooks. Drop `--apply` to print the snippet instead of mutating.
For Claude Code, `CLAUDE_CONFIG_DIR` relocates MCP registration to
`$CLAUDE_CONFIG_DIR/.claude.json`, hooks to
`$CLAUDE_CONFIG_DIR/settings.json`, and global managed skills to
`$CLAUDE_CONFIG_DIR/skills`. The Docker wrapper forwards this variable when
the directory is under its existing `$HOME` bind mount. Use the native binary
when the Claude config root is outside `$HOME`. Uninstall checks both the
active relocated paths and Claude's home defaults, so enabling the variable
does not leave an older default-path ai-memory installation behind.
If your agent often starts inside repository subdirectories or linked
worktrees, add `--project-strategy repo-root` to `install-hooks` so captures
collapse to the main git repo name; see [`docs/install.md`](docs/install.md)
and [`docs/marker-file.md`](docs/marker-file.md) for details. Later bare
`--apply` refreshes, including `ai-memory upgrade`, preserve that choice;
pass `--project-strategy basename` explicitly to remove it.

The Docker wrapper also bridges thin-client commands such as
`ai-memory status` and `ai-memory bootstrap` back to the host's
loopback server. With the local Docker quick start above, no
`AI_MEMORY_SERVER_URL` override is needed.

Managed workstreams are optional. They execute the harness on the host while
the server may remain local or remote:

```bash
ai-memory run claude
ai-memory run codex --yolo   # later: same workstream, different harness
ai-memory continue           # resume the newest managed checkout
```

`ai-memory uninstall --apply` removes everything ai-memory installed,
and only what it installed. Install commands are idempotent and write
timestamped backups next to any file they touch.

## Everyday use

Day to day, you mostly do not think about ai-memory. Hooks capture
prompts, tool calls, and session boundaries; session end turns them into
readable wiki pages; the next session starts with a handoff.

- Ask "where did we leave off?" to continue from the pending handoff.
- Ask "have we discussed X?" or "search memory for Y" to query the wiki.
- Ask "catch me up" for a prose digest of recent project activity.
- Run `ai-memory bootstrap` once when adopting an existing project with
  months of history.
- Start the server with `--enable-web` for a read-only browser view of
  the wiki and a JSON API under `/api/v1`.

The full tour — search modes, entities, feedback, briefings, the web
API — is in [`docs/usage.md`](docs/usage.md) and
[`docs/use-cases.md`](docs/use-cases.md).

## Teams and multiple machines

Run the server somewhere reachable — a homelab box, a LAN host — and
point every machine and every teammate at it. Knowledge is shared per
project; personal handoffs stay personal; every write is attributed and
audited. Multi-user auth (passwords, API credentials) is built in.

Start with [`docs/users.md`](docs/users.md) for accounts and ownership,
and [`docs/deploy.md`](docs/deploy.md) for the server itself — including
capacity numbers measured rather than guessed, and the one rule that
matters: one server per data directory, never two.

## Security

The quick-start default is loopback-only with no auth — nothing outside
your machine can reach it. From there, hardening is incremental: a bearer
token for the LAN, per-user accounts, OIDC device auth for hooks, TLS via
a reverse proxy. Capture is sanitized at a typed privacy boundary before
anything is stored, and per-repository `[capture]` rules can exclude
paths or invert to allowlist mode.

The full model is in [`docs/security.md`](docs/security.md),
[`docs/users.md`](docs/users.md), and
[`docs/https-via-proxy.md`](docs/https-via-proxy.md).

## LLM providers

Optional. Everything works with zero LLM calls; adding a provider
upgrades session summaries and enables semantic search. Anthropic,
OpenAI (incl. OAuth/Codex), GitHub Copilot, Gemini, OpenCode Zen, and
any OpenAI-compatible endpoint (Ollama, LM Studio, vLLM) are supported
for consolidation; OpenAI, Voyage, Gemini, and keyless OpenAI-compatible
endpoints for embeddings. Configuration lives in
[`docs/llm-providers.md`](docs/llm-providers.md).

## Architecture

One Rust binary runs an MCP/HTTP server and owns one data directory:

```text
<data_dir>/
├── wiki/    # markdown source of truth, git-versioned
├── raw/     # immutable sanitized managed-workstream transcript segments
├── db/      # SQLite indexes, including FTS5, entities, and embeddings
├── models/  # reserved for local embedding models
└── logs/    # rolling tracing output
```

Hooks POST observations to the server. The server serializes writes
through one SQLite writer, compiles session observations into markdown
pages, and serves retrieval through FTS5, entity-match and graph-neighbor RRF,
optional vector RRF, bounded source-authority adjustment, and bounded
raw-observation fallback for non-global searches.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the data-flow
diagram, crate breakdown, schema notes, and invariants.

## Docs

| File | What it is |
|---|---|
| [`docs/install.md`](docs/install.md) | **Installation cookbook.** Every agent CLI, every alternative (curl, source build, no-docker, no-auth), and the server-on-a-different-machine (homelab/LAN) walkthrough. Read after the Quick start if your setup doesn't match the happy path. |
| [`docs/usage.md`](docs/usage.md) | Handoffs, proactive memory queries, slim routing snippet + managed Agent Skills, migration from other memory tools, web UI, raw-wiki inspection, and rules-vs-facts workflow. |
| [`docs/managed-workstreams.md`](docs/managed-workstreams.md) | Optional `ai-memory run` continuity across Claude Code, Codex, OpenCode, Pi, Crush, Kimi Code, Command Code, Kiro CLI v2/v3, OMP, Grok Build CLI, and Antigravity CLI: automatic harness selection, native resume, argument forwarding, ledger search, privacy, and recovery. |
| [`docs/managed-harness-contributions.md`](docs/managed-harness-contributions.md) | Protocol and acceptance bar for contributors adding managed resume, read-only transcript import, and startup context delivery to another harness. |
| [`docs/marker-file.md`](docs/marker-file.md) | `.ai-memory.toml` workspace/project routing for multi-client trees, mono-repos, worktrees, and work/personal separation. |
| [`docs/auto-scope.md`](docs/auto-scope.md) | `[auto_scope]` modes for shared servers: default single-slot routing, session-aware isolation, and multi-user `per_actor` behavior. |
| [`docs/macos.md`](docs/macos.md) | macOS install paths: native release binary (recommended), source build, the Docker wrapper, hook-platform notes, and current macOS limitations. |
| [`docs/windows.md`](docs/windows.md) | Windows install modes: full WSL2, native Windows with Docker Desktop, prebuilt native release zip, native source builds, and current hook/MCP harness caveats. |
| [`docs/mcp-install.md`](docs/mcp-install.md) | Per-client MCP and lifecycle notes, handoff-injection limits, and community bridge guidance. |
| [`docs/deploy.md`](docs/deploy.md) | Homelab deploy: bin/deploy, bearer-token auth, pointers to the TLS guide. |
| [`docs/users.md`](docs/users.md) | **Multi-user attribution and human login.** Four-rung bearer ladder, password sessions, `ai-memory user` / `api-key` walkthrough, brownfield `aim_` migration. |
| [`docs/https-via-proxy.md`](docs/https-via-proxy.md) | **HTTPS via a reverse proxy.** When you need TLS (multi-user, non-loopback) and when you don't (loopback / stdio). Copy-paste docker compose templates for Caddy + Let's Encrypt, Caddy + internal CA (LAN-only), Cloudflare Tunnel (no open ports), and external cert files; plus native-Caddy + nginx recipes. The "thinking you're secure when you're not" failure modes explicitly called out. |
| [`docs/lifecycle-ops.md`](docs/lifecycle-ops.md) | **Read before running purge / rename / backup / restore / reset / reindex / restore-page.** Safety matrix for state-touching commands, per-project disk layout (how isolation actually works), checkpoint-based page recovery, and operator workflows for "fresh start", "snapshot before risky op", "drop one project", and rebuilding SQLite from wiki files. |
| [`docs/auto-improvement-loop.md`](docs/auto-improvement-loop.md) | Auto-improvement design notes: Hermes-inspired scheduled review, auto-approval default, manual review opt-in, pending proposal storage, and curator work. |
| [`docs/companion-crates.md`](docs/companion-crates.md) | Boundary and implementation plan for optional companion projects, including the standalone importer at [`companions/ai-memory-importer`](companions/ai-memory-importer), without widening core ai-memory. |
| [`docs/llm-provider-comparison.md`](docs/llm-provider-comparison.md) | Empirical notes behind the recommended LLM defaults. |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Operational summary: data flow, crate layout, cross-cutting invariants, schema. |
| [`docs/design-decisions.md`](docs/design-decisions.md) | The full v1 spec. |
| Research docs under `docs/` | Karpathy LLM Wiki notes, Hermes Agent, agentmemory / basic-memory / cognee deep-dives, lessons-learned from upstream issues. |
- [`docs/support-matrix.md`](docs/support-matrix.md) - the full agent/platform matrix with notes.
- [`docs/use-cases.md`](docs/use-cases.md) - scenario walkthroughs.
- [`docs/llm-providers.md`](docs/llm-providers.md) - provider configuration.
- [`docs/security.md`](docs/security.md) - the full security model.
- [`docs/research-2026-landscape.md`](docs/research-2026-landscape.md) - how the field looks and where we sit in it.
- [`docs/ROADMAP-2.0.md`](docs/ROADMAP-2.0.md) - the plan for the 2.0 release, one item at a time.
- [`docs/okf.md`](docs/okf.md) - the wiki is natively an Open Knowledge Format (OKF v0.2) bundle; design and field mapping.
- [`docs/typed-edges.md`](docs/typed-edges.md) - typed relation edges (`causes` / `fixes` / `contradicts`) and how lint uses them.
- [`docs/temporal.md`](docs/temporal.md) - ingestion-time validity on the entity index and `as_of` time-travel queries.
- [`docs/local-embeddings.md`](docs/local-embeddings.md) - in-process embeddings with no API key (`embedding_provider = "local"`).
- [`docs/experience.md`](docs/experience.md) - the opt-in cross-session abstraction pass: knowledge visible only across trajectories.
- [`docs/MIGRATION-2.0.md`](docs/MIGRATION-2.0.md) - upgrading an existing store to 2.0: the backup-gated automatic migration and how to restore.
- [`docs/benchmarks/`](docs/benchmarks/README.md) - published retrieval-quality numbers with provenance, reproducible from the in-repo harness.

## Influences and prior art

- **[Karpathy LLM Wiki](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f)** - the compile-not-retrieve pattern.
- **[agentmemory](https://github.com/rohitg00/agentmemory)** - most of the right ideas; this project is the Rust successor.
- **[basic-memory](https://github.com/basicmachines-co/basic-memory)** - the markdown-on-disk source-of-truth model.
- **[cognee](https://github.com/topoteretes/cognee)** - pipeline composition and triplet embeddings.
- **[Hermes Agent](https://github.com/NousResearch/hermes-agent)** - the self-improvement loop: post-turn review, approval gates, and curator boundaries.
- **[A-MEM](https://arxiv.org/abs/2502.12110)** - Zettelkasten-style atomic notes with link evolution.

## License

MIT - see [LICENSE](LICENSE).

## Acknowledgements

This codebase is being built collaboratively with Claude Code
(Anthropic Claude Opus 4.7) following the plan documented in
`docs/design-decisions.md`.
