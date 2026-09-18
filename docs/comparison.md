# How ai-memory compares

For anyone evaluating ai-memory against another agent-memory tool — or
migrating from one. This page aims to be **fair and specific**: what each
approach does well, where ai-memory differs, where others are ahead, and how
the field has independently validated the bets ai-memory made. The deep
analysis behind it is in [`research-2026-landscape.md`](research-2026-landscape.md)
and the per-project research docs; the numbers are from
[`benchmarks/`](benchmarks/README.md), reproducible from the in-repo harness.

## The short version

Most memory tools optimize one of: extracting atomic **facts** per turn
(Mem0, LangMem), a temporal **knowledge graph** (Zep/Graphiti), an
agent-editable **memory OS** (Letta, MemOS), or a hosted **context database**
(OpenViking). ai-memory optimizes something different: **a git-backed markdown
wiki as the source of truth, with a derived index for retrieval**, captured
automatically from lifecycle hooks, shared across agents and machines.

What actually distinguishes it:

- **Cross-agent and cross-machine by construction.** One server; 20+ harnesses
  (Claude Code, Codex, Cursor, Gemini, OpenCode, Kimi, …) feed and read the
  same memory. Quit one agent, open another in the same repo, get a real
  handoff. Handoffs are a typed protocol (owned, claimed exactly once), not a
  note file.
- **Zero-LLM default.** Capture, FTS5 + entity + graph retrieval, local
  embeddings, and rule-based summaries all work with **no API key**. An LLM is
  opt-in for richer consolidation — not a requirement to function.
- **Files are the truth.** The wiki is plain markdown + YAML frontmatter — a
  native [Open Knowledge Format](okf.md) (OKF v0.2) bundle. `grep` it, open it
  in Obsidian, edit by hand, `rsync` it. The SQLite index is derived and
  rebuildable. Nothing is trapped in a vector store or binary blob.
- **Multi-user without a paid tier.** An auth ladder (root → DB-user tokens →
  OIDC), per-person attribution, audit log, and the invariant that **pages are
  shared per project while handoffs stay owned** — a team story built in.
- **One self-contained binary.** Bundled SQLite, vendored libgit2; no external
  services to stand up. Runs on a laptop, a homelab box, or a LAN server.

And it publishes numbers: **LongMemEval-S hit@5 0.823** (local-embeddings
default; 0.668 zero-LLM), produced by the in-repo harness with full provenance
— not a marketing claim. For context on the same dataset, mcp-memory-service
reports 0.804 R@5 and agentmemory 0.967 R@5 (hybrid + reranking); ai-memory is
comparable to the former and honestly below the latter, and it documents *why*
(the pipeline pays a real 2 KB privacy-capture cost the raw-log retrievers do
not). See [where we're behind](#where-were-behind-or-different-by-choice).

## By camp

| Approach | Representatives | Strength | Trade-off vs ai-memory |
|---|---|---|---|
| Fact extractors | Mem0, LangMem | Cheap per-turn personalization | Atomic facts lose relational/causal context (see [TriMem](research-2026-landscape.md#4-research-developments-worth-knowing)); LLM-per-turn; not file-first |
| Hosted memory API (hybrid) | **Supermemory** | Chunk-RAG + LLM temporal fact-graph + per-user profiles in one query; managed connectors (Drive/Notion/GitHub), multimodal, metadata/tag query filters | Cloud-first (best features + extraction are paid/Cloudflare-hosted); LLM-required quality path; an opaque graph is the source of truth (not file-first, nothing to `grep`/diff); the MIT self-host binary drops the connectors and extraction models |
| User-modeling / theory-of-mind | **Honcho** (Plastic Labs) | Reasoning-derived model of what each "peer" knows/believes over time — personalizes around the *human* the agent serves | Different problem: it remembers the *user*, ai-memory remembers the *project*. Opaque Postgres, LLM-required (Deriver/Dreamer), multi-service stack; adjacent (shares MCP/plugin delivery) but not a coding-memory migration target |
| Temporal knowledge graph | Zep/Graphiti, Cognee | Bi-temporal "what was true vs believed when" | Needs a graph DB; heavier to self-host. ai-memory ships **bi-temporal-lite** on SQLite ([`temporal.md`](temporal.md)) + typed edges ([`typed-edges.md`](typed-edges.md)) for the useful part |
| Memory OS / self-editing | Letta, MemOS, MIRIX | Agent curates its own tiered memory | Token-expensive self-editing; Letta itself now concedes file-first ("Is a Filesystem All You Need?") |
| Hosted context database | **OpenViking** (ByteDance) | Progressive L0/L1/L2 loading; directory-scoped retrieval; broad integrations | LLM-**required** (VLM + embeddings); opaque swappable storage; AGPLv3 core + SaaS/enterprise weight |
| Paper-backed pages | **Hindsight** (Vectorize) | "Mental models" = living markdown pages an agent boots from; belief-strength consolidation; a preprint | Postgres/pgvector-primary, LLM-required; strict per-bank isolation (no team-sharing within a project) |
| Closest sibling (fact-row twin) | doobidoo/mcp-memory-service | SQLite(+vec), local ONNX, hook capture, typed edges, honest numbers | What ai-memory would be if it chose fact-rows over wiki **pages** |
| Platform-native | Claude Code auto-memory | Zero setup, on by default | Machine-local, **no sync**, single-agent, repo-scoped, no tool-lifecycle capture, no team |
| **File-first wiki (ai-memory)** | ai-memory, basic-memory, OKF | Human-editable markdown truth + derived index; cross-agent; zero-LLM default; multi-user | Below the reranking leaders on raw R@5; LLM-optional means no VLM fact-extraction sophistication |

## How the field validates the approach

The strongest endorsement of ai-memory's design is that others arrived at its
core bets independently, from different substrates:

- **Google standardized file-first.** The [Open Knowledge Format](okf.md) (June
  2026) — markdown + YAML frontmatter, one concept per file, no runtime — is
  the interop layer for agent memory. ai-memory's wiki is a native OKF bundle.
- **Letta conceded the filesystem.** The "memory OS" camp's own leader
  published that agents post-trained for file search rival specialized memory
  systems — the file-as-truth + derived-index split ai-memory uses.
- **Hindsight's paper validates pages-over-facts.** A funded competitor on the
  *opposite* (Postgres, LLM-required) substrate makes its top tier "mental
  models" — living markdown pages a background process rewrites and the agent
  boots from. That is ai-memory's `wiki/` + [auto-improve loop](auto-improvement-loop.md)
  under another name, backed by arXiv:2512.12818.
- **OpenViking validates document memory + a consolidation loop.** ByteDance's
  entrant stores document/directory-scoped memory (not atomic facts) with a
  background extract/merge pass and a "compile" step into wikis — the same
  shape as ai-memory's consolidation, from an LLM-required corner.
- **The literature backs it.** TriMem (arXiv:2605.19952) argues document/page/
  narrative hierarchies beat atomic-fact stores; the Storage→Reflection→
  Experience survey (arXiv:2605.06716) names cross-trajectory abstraction as
  the frontier — which ai-memory's [experience pass](experience.md) targets.

ai-memory also **shipped the borrowable ideas** from that research rather than
just cataloguing them: typed relation edges, ingestion-time temporal validity
with `as_of` queries, local (no-key) embeddings as the default, and the
cross-session abstraction pass are all in the product today.

## Coming from another tool?

- **From Mem0 / a fact extractor:** you keep automatic capture, but memory
  compiles into readable **pages** you can open and edit, not opaque fact rows.
  Retrieval fuses FTS + entity + graph + vectors instead of vector-only.
- **From Zep/Graphiti:** you get bi-temporal-lite (`as_of`, version-filtered
  search) and typed edges without standing up a graph database — on one binary.
- **From Claude Code's built-in memory:** the same "remember my project"
  convenience, but synced across machines and agents, searchable, team-capable,
  and capturing tool lifecycle — not a per-laptop `MEMORY.md`.
- **From mcp-memory-service:** a very close sibling; the switch is fact-rows →
  wiki pages (human-editable markdown truth) and cross-agent handoffs as a
  first-class protocol. Cross-project [agent messaging](agent-messaging.md) is
  new ground neither had as a typed queue.
- **From Hindsight / OpenViking:** you trade a hosted, LLM-required service for
  a self-contained binary that runs zero-LLM by default and keeps memory in
  files you own. You give up (for now) their VLM-driven extraction depth and
  their published headline accuracy numbers; you gain no vendor lock-in, no
  required API spend, and per-project team sharing rather than strict per-bank
  isolation.

## Where we're behind, or different by choice

Fair means saying this plainly:

- **Raw retrieval score.** 0.823 hit@5 on LongMemEval-S is comparable to
  mcp-memory-service and below agentmemory's 0.967 (hybrid + reranking). Part
  is deliberate — a 2 KB privacy cap on captured excerpts puts evidence deep
  inside one long turn out of the index's reach; the benchmark measures the
  *shipped, sanitized* system, not an idealized retriever.
- **Headline benchmark comparability.** Hindsight quotes 91.4% *accuracy* and
  OpenViking quotes LoCoMo lifts — different datasets/metrics than our hit@5,
  and both are self-reported/preprint. We publish a reproducible harness and a
  single stated metric rather than a bigger number.
- **No VLM fact extraction.** Because the default path is zero-LLM, ai-memory
  does not do the LLM-per-turn atomic extraction the fact-extractor and
  LLM-required systems build on. Consolidation is opt-in and page-shaped.
- **Not a graph database.** ai-memory chooses bi-temporal-lite on SQLite over a
  full temporal knowledge graph — the useful 80%, not the graph-query surface.
- **Single server, not SaaS.** No hosted multi-region tier, no enterprise
  console. That is the point (own your data, one binary), but it is a
  difference if you want managed infrastructure.

If a specific comparison here reads as unfair or out of date, open an issue —
these numbers and claims are meant to be checkable against the linked research
and the reproducible harness.
