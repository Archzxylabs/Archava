# Local Laya decision-model trial — 9 November 2026

The user requested a practical Jev/Laya experiment to assess whether a decision
model could help development. Laya was selected for a finite local trial without
a new API key. **Both tested configurations failed the provisional advisory
screen.** They are potentially useful for suggestions and triage, but this trial
does not justify automatic approval, a replacement planner, or a claim that
Archava development became faster. Jev was not tested.

## Frozen scope and setup

The initial dataset has **36 hand-authored synthetic cases**: six module-routing
requests and twelve evidence/reply comparisons, each in English and Indonesian.
The comparisons cover verified changes, pending approval, partial completion,
negated refund instructions, an unexecuted proposal, provider failure, mismatched
facts, pagination, revoked access, metadata-only history, and clearing versus
bank evidence. The labels were frozen before inference. They describe the
explicit fixture facts, not an actual payment or application write.

Each case was evaluated twice in original and reversed choice order: **144
predictions, not 144 independent business cases**. Questions and labels were not
tuned after seeing results. An exploratory second run used the same eighteen
English cases and identical questions with the English checkpoint, producing
72 additional predictions. This second comparison was selected after inspecting
the multilingual result and is reported separately.

- SDK: `laya==0.4.30`; wheel SHA-256
  `d9408153b234d90c5729de69ed4cb8486979a812d6ae797bdf044d9e57c93288`.
- Multilingual revision: `1720e3e3357cfe1e281542e223f8273b0890ca34`.
- English revision: `85cc74ddae6109077ebe2853a9b3077fb6606a78`.
- CPython 3.12.13, Torch 2.9.1+cpu, Transformers 5.22.2; four CPU threads,
  eager mode, batch size one, 1024-token state/sequence budget and 256-token
  question-head budget. The actual encoded inputs lost no state tokens or
  choice descriptions. No calibration was fitted to these evaluation cases.
- Intel i7-11800H; **the RTX 3050 Ti was not used**. Other user processes and
  a separate build were present. Timing measures local observed latency,
  not isolated hardware performance or GPU speed.

Public packages and pinned model artifacts were downloaded into a task-owned
environment under `target/laya-trial-20261109-01`. Inference then ran with a clean
environment, Hugging Face/Transformers offline mode and a Python network audit
guard; both runs recorded zero network attempts. No API key or customer data
was used. No inference server, MCP registration, hook or startup entry was added.

## Measured results

| Configuration and dataset | Correct predictions | Original-order first pass | Warm p50 / p95 |
| --- | --- | --- | --- |
| Multilingual, English + Indonesian | 124/144 (86.1%) | 31/36 | 260 / 324 ms |
| English, English only | 56/72 (77.8%) | 14/18 | 599 / 726 ms |

The table's datasets differ. On the **same English subset**, multilingual scored
60/72 (83.3%) versus English's 56/72 (77.8%). These eighteen cases are insufficient
to establish general model superiority.

The multilingual group results were routing English 22/24, routing Indonesian
20/24, reply evidence English 38/48, and reply evidence Indonesian 44/48. English
scored 20/24 on routing and 36/48 on reply evidence. Repeated identical requests
agreed 100% in both runs. Reversing choice order agreed 66/72 pairs (91.7%) for
multilingual and 36/36 for English. Stable repetition did not establish correctness.

Multilingual incorrectly accepted an unsupported or explicitly conflicting reply
in **8/96 semantic predictions across three distinct cases**; English did so in
4/48 across one distinct case. In the synthetic refund counterexample,
multilingual accepted “No refund was issued” even though a verified receipt
explicitly recorded a refund; reported answer confidence reached **0.969**.
An ambiguous Indonesian request was also confidently assigned to Purchasing.
A high reported confidence therefore cannot establish a safe approval threshold.

The predeclared exploratory screen required at least 90% accuracy in every
family/language group, 95% option-order agreement and 99% repeat agreement.
Neither configuration passed. These were experiment thresholds, not a new
product release gate. Peak process RSS was approximately 2265 MiB and 2737 MiB;
both finite inference processes exited successfully and released their models.

## Evidence and decision

Local raw evidence is in `target/laya-trial-20261109-01`, with the English
comparison in its `english/` subdirectory. `cases.json`, `questions.json`,
`trial-plan.json`, `model-manifest.json`, `requirements.freeze.txt`, raw
`predictions.jsonl`, encoding checks, result JSON and execution logs are retained.
`verify_results.py` independently checked the complete prediction matrix, labels,
confusion counts, latency statistics, order/repeat pairs and fixture hashes.
Both verification runs passed artifact/accounting checks; model quality remained
failed. Local evidence is build output and is not included in native packages.

A final all-files equality check initially found the SDK's own multilingual
tokenizer compatibility normalization: `extra_special_tokens` changed from a
list to a named mapping before tokenization. Comparing both JSON objects proved
that was the only change; model weights and evaluation inputs/results were
unchanged. The effective runtime config and this finding are retained in
`runtime-tokenizer-config.json` and `runtime-compatibility.json`. The original
downloaded config was restored after inference so future loads can verify the
same published hashes again. This normalization is not a model or prompt tune.

- Initial cases SHA-256:
  `60f2e90755ed3d479d21b973ce8f4be101184918654b007e19f3123d88af8913`.
- Multilingual result SHA-256:
  `0f4f4e2682d8a0f66fde0e2e7fe9593eac461e1fe95380e72d6ed4eea2fd1d2b`.
- English result SHA-256:
  `98317ecd4345d45f099a7060058bab32ed9f36228b074d244861f0f0a8ed5c25`.

Keep Laya available for explicitly invoked advisory experiments. Its first
candidate role is suggesting routing or highlighting replies for human review.
SQL oracles, authorization, approval and exact financial calculations remain
authoritative. No application integration was made, and no PRD requirement was
accepted by this experiment. The current 516-input product source remains
`7d23728e3c7eda273f200d057e16b319f984e88c781897ed2583713633f44c81`.
The separate [real-Gemini acceptance remains open](PRD_CRM_READ_FINDING_2026-11-09.md).

No Gemini/Jev head-to-head, coding quality, compilation time, developer-time or
end-to-end product speedup was measured. The small authored cases, two choice
orders and lack of independent production labels limit the conclusions.

Primary setup references: [Laya repository](https://github.com/NandhaKishorM/laya)
and [published multilingual checkpoint](https://huggingface.co/convaiinnovations/laya-multilingual).
