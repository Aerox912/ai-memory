# AI Memory 2.6.6-aerox.1 retrieval review

The full LongMemEval-S run scored 470 questions in each configuration and excluded the same 30 abstention questions. It used the default eight-way concurrency, k=1/3/5/10, FTS without embeddings, and local all-MiniLM-L6-v2 without a reranker. QA and live LLM grading were off.

- Tested source: `a9771806f8b387b3dee8ee5e342123e838a7e289`, the commit tagged `v2.6.6-aerox.1`.
- Dataset SHA-256: `08d8dad4be43ee2049a22ff5674eb86725d0ce5ff434cde2627e5e8e7e117894`.
- Host: Intel(R) Core(TM) Ultra 9 275HX (24 threads), Arch Linux under WSL, existing Rust 1.99.0 release build, temporary database/wiki on WSL's existing tmpfs.
- Released Linux/Windows assets use the separately verified Rust 1.95 build workflow. These benchmark timings measure the source build on this host, rather than the deployed services.
- [Machine report](longmemeval-s-2026-10-13-aerox-2.6.6.json) and [all available historical slice comparisons](longmemeval-s-2026-10-13-aerox-2.6.6-comparison.csv).

## Release review

FTS reproduces every published 2.4 overall accuracy figure after rounding: hit@5 0.666 and recall@5 0.536. Local hit@5 is 0.821 and recall@5 is 0.680. The local hit@5 result matches the alternate full 2.4 run already documented in [the published A/B report](retrieval-ab-r2.md).

Against the first published 2.4 run, local hit@3 moves from 0.730 to 0.723, a three-question difference. Hit@5 improves from 0.815 to 0.821, recall@5/10 improves, and hit@1 is unchanged. Hit@3 is only 0.003 below the older full-slice 2.0 result. The assistant hit@5 slice moves by one question (0.821 to 0.804); every other recent slice is stable or higher within the previously documented small-slice variation. No accuracy drop against the older full-slice reports exceeds 0.03. These differences were reviewed as run/ranking variation, without evidence of a sustained default-ranking regression.

The complete tables below retain all k values and all slices. The comparison CSV includes every accuracy field available from the older full-slice reports, with its baseline labelled explicitly.

Current FTS query p50/p95 is 12/80 ms and local is 98/214 ms. The older published Ryzen 7950X3D 32-thread run measured 5/40 and 94/162 ms. Hardware, compiler, storage and OS differ, so this is not a controlled version comparison or a deployed-service latency claim. FTS estimated context mean/median is 350.0/315.5 tokens versus 350.3/316.5; local is 419.6/375.5 versus 415.7/367.5. Context uses the harness's chars/4 estimate over returned titles and snippets.

Top-five-only context and pure ingest throughput are not emitted by this harness, and matching historical measurements are unavailable. Those comparisons remain unknown. End-to-end replay timing includes queries and scoring and cannot substitute for pure ingest throughput.

All 23 mandatory source CI jobs, three native Windows jobs, and two Nix jobs passed on the exact tagged commit. The full benchmark completed successfully on this WSL host. The slower duplicate GitHub benchmark was cancelled after that result; its cancellation does not represent a source test failure. See [CI platform jobs](https://github.com/Aerox912/ai-memory/actions/runs/38396651696), [Windows](https://github.com/Aerox912/ai-memory/actions/runs/38396655738), and [Nix](https://github.com/Aerox912/ai-memory/actions/runs/38396659615).

## Harness output

# LongMemEval-S retrieval A/B: 2026-10-13T15:15:24.849353155Z

- commit: `a9771806f8b387b3dee8ee5e342123e838a7e289`
- dataset: `longmemeval_s` (sha256 `08d8dad4be43ee20…`)
- hardware: Intel(R) Core(TM) Ultra 9 275HX (24 threads)
- questions scored: 470 (30 abstention questions excluded)
- baseline: embeddings=none reranker=off (zero-llm)
- candidate: embeddings=local reranker=off (local-embeddings)

## Baseline

| slice | n | hit@1 | hit@3 | hit@5 | hit@10 | recall@1 | recall@3 | recall@5 | recall@10 | p50 ms | p95 ms | ctx tok mean | ctx tok med |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| knowledge-update | 72 | 0.583 | 0.750 | 0.764 | 0.778 | 0.292 | 0.549 | 0.562 | 0.590 | 17 | 72 | 298.3 | 234.0 |
| multi-session | 121 | 0.446 | 0.554 | 0.579 | 0.603 | 0.193 | 0.350 | 0.385 | 0.403 | 8 | 22 | 272.6 | 216.0 |
| overall | 470 | 0.532 | 0.645 | 0.666 | 0.694 | 0.349 | 0.508 | 0.536 | 0.564 | 12 | 80 | 350.0 | 315.5 |
| single-session-assistant | 56 | 0.679 | 0.786 | 0.804 | 0.839 | 0.679 | 0.786 | 0.804 | 0.839 | 74 | 113 | 589.4 | 628.0 |
| single-session-preference | 30 | 0.300 | 0.467 | 0.533 | 0.667 | 0.300 | 0.467 | 0.533 | 0.667 | 24 | 42 | 603.9 | 633.0 |
| single-session-user | 64 | 0.625 | 0.656 | 0.688 | 0.703 | 0.625 | 0.656 | 0.688 | 0.703 | 4 | 8 | 303.2 | 260.5 |
| temporal-reasoning | 127 | 0.528 | 0.646 | 0.654 | 0.669 | 0.256 | 0.448 | 0.470 | 0.485 | 13 | 62 | 311.2 | 267.0 |

## Candidate

| slice | n | hit@1 | hit@3 | hit@5 | hit@10 | recall@1 | recall@3 | recall@5 | recall@10 | p50 ms | p95 ms | ctx tok mean | ctx tok med |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| knowledge-update | 72 | 0.611 | 0.806 | 0.903 | 0.958 | 0.306 | 0.583 | 0.743 | 0.889 | 108 | 198 | 387.9 | 346.0 |
| multi-session | 121 | 0.463 | 0.744 | 0.876 | 0.950 | 0.201 | 0.451 | 0.630 | 0.814 | 78 | 136 | 377.1 | 344.0 |
| overall | 470 | 0.536 | 0.723 | 0.821 | 0.889 | 0.345 | 0.564 | 0.680 | 0.818 | 98 | 214 | 419.6 | 375.5 |
| single-session-assistant | 56 | 0.732 | 0.804 | 0.804 | 0.821 | 0.732 | 0.804 | 0.804 | 0.821 | 182 | 327 | 612.0 | 630.5 |
| single-session-preference | 30 | 0.433 | 0.667 | 0.767 | 0.900 | 0.433 | 0.667 | 0.767 | 0.900 | 88 | 146 | 604.5 | 633.5 |
| single-session-user | 64 | 0.453 | 0.656 | 0.750 | 0.812 | 0.453 | 0.656 | 0.750 | 0.812 | 60 | 92 | 354.1 | 302.0 |
| temporal-reasoning | 127 | 0.543 | 0.669 | 0.780 | 0.858 | 0.259 | 0.485 | 0.581 | 0.764 | 115 | 194 | 382.5 | 351.0 |

## Delta (overall, candidate − baseline)

| metric | baseline | candidate | delta |
|---|---|---|---|
| hit@1 | 0.532 | 0.536 | +0.004 |
| hit@3 | 0.645 | 0.723 | +0.079 |
| hit@5 | 0.666 | 0.821 | +0.155 |
| hit@10 | 0.694 | 0.889 | +0.196 |
| recall@1 | 0.349 | 0.345 | -0.003 |
| recall@3 | 0.508 | 0.564 | +0.056 |
| recall@5 | 0.536 | 0.680 | +0.144 |
| recall@10 | 0.564 | 0.818 | +0.254 |
| latency p50 ms | 12 | 98 | +86 |
| latency p95 ms | 80 | 214 | +134 |
| ctx tok mean | 350.0 | 419.6 | +69.6 |
| ctx tok median | 315.5 | 375.5 | +60.0 |

Notes: hit@k = any evidence session in top k (the "Recall@k" most systems publish); recall@k = fraction of evidence sessions found. Latency is the `memory_query` MCP round trip (send→parse); ctx tok is the estimated context an agent ingests from the result = chars/4 over returned hit titles + snippets. Session attribution: `sessions/<id>.md` pages and raw observation hits; unattributable pages never score. Capture is production-shaped: excerpts bounded at the 2 KB privacy boundary.
