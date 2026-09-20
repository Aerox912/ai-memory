# Retrieval A/B (R2) — accuracy + latency + context-tokens triple

The `retrieval` harness can compare two server/query configurations over
the **same** LongMemEval-S question set in one run, reporting a triple per
config and a baseline→candidate delta:

- **accuracy** — `hit@k` / `recall@k` (as the single-config baselines do);
- **latency** — `memory_query` MCP round-trip p50 / p95, milliseconds;
- **context tokens** — the context an agent would ingest from a result,
  estimated as **chars / 4** over every returned hit's `title` + `snippet`
  (a documented, provider-agnostic heuristic — not a real tokenizer).

Provenance (commit, dataset sha256, hardware, per-config knob summary) is
carried exactly as the single-config reports carry it. See
`evals/README.md` for the full flag reference.

The harness can additionally run **end-to-end QA-accuracy** (`--qa`, #771/#772):
each question's retrieved context is handed to an LLM that answers it, and an
LLM-as-judge grades the answer against the gold label. This reports
answered/graded accuracy, answer latency (p50/p95), and answer-token cost
alongside the retrieval triple. It requires a provider key and is off by
default.

## How to run

```bash
cargo build --release -p ai-memory-cli
# baseline (FTS-only) vs candidate (local embeddings), full 500 questions:
cargo run --release -p ai-memory-eval -- retrieval --fetch \
    --candidate-embeddings local
```

A run without a candidate config is unchanged and still writes the
single-config report. A baseline-vs-baseline run (`--candidate`, no
differing knob) is the determinism check: accuracy and context-token
deltas are exactly zero (latency wobbles at wall-clock noise).

## Published A/B numbers

> Full-dataset (500-question) A/B baselines go here, one section per
> comparison, pasted verbatim from `evals/runs/<stamp>-retrieval/report.md`
> with its provenance header. **Not yet populated** — running the full
> matrix is a deliberate, separately-scheduled pass (the dataset is 278 MB
> and a full local-embeddings leg is minutes, not seconds). Do not paste
> `--sample` numbers here as if they were baselines.

### Illustrative smoke (NOT a baseline)

Recorded only to show the report shape. `--sample 4`, four
`single-session-user` questions, so the accuracy figures are noise;
latency and context-tokens show the expected direction (local embeddings
add query latency):

| metric | baseline (none) | candidate (local) | delta |
|---|---|---|---|
| hit@5 | 0.750 | 0.500 | -0.250 |
| latency p50 ms | 2 | 31 | +29 |
| latency p95 ms | 2 | 35 | +33 |
| ctx tok mean | 408.5 | 393.2 | -15.2 |
| ctx tok median | 494.5 | 389.0 | -105.5 |

Hardware: AMD Ryzen 9 7950X3D (32 threads). Replace this section with a
full-dataset run before citing any A/B number.

### Illustrative sample-20 run (triple + QA-accuracy, NOT a baseline)

Recorded to show the triple + QA columns on a real run. **`--sample 20`, 20
LongMemEval-S questions — illustrative, NOT the full-500 baseline.** The
published baseline stays 0.823 hit@5 (local) / 0.668 zero-LLM in
[README.md](README.md); do not cite the numbers below as a baseline.

Retrieval (zero-LLM):

| metric | value |
|---|---|
| hit@1 | 0.750 |
| hit@5 | 0.800 |
| hit@10 | 0.800 |
| recall@5 | 0.800 |
| latency p50 / p95 ms | 2 / 3 |
| ctx tok mean / median | 330.2 / 386.5 |

QA-accuracy (Gemini `gemini-2.5-flash` answered + graded):

| metric | value |
|---|---|
| QA accuracy | 9/20 = 0.450 |
| answer latency p50 / p95 ms | 705 / 862 |
| answer tokens mean | 453.9 |

Reproduce (QA needs `GEMINI_API_KEY`):

```bash
cargo run --release -p ai-memory-eval -- retrieval \
    --sample 20 --qa --qa-provider gemini --qa-model gemini-2.5-flash
```
