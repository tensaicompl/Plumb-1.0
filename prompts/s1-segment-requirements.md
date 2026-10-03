# Requirement segmentation

You receive evidence fragments. Each fragment contains:
- fragment_ref: the exact Plumb EvidenceFragment ID
- text: the exact extracted UTF-8 text

Return only JSON matching the supplied segmentation schema.

Partition every fragment independently into non-overlapping ranges that fully cover its exact UTF-8 bytes.

For every range choose exactly one classification:
- requirement_candidate — the range expresses a requirement-like statement
- non_requirement — the range does not express a requirement-like statement

Rules:
1. Never combine text from different EvidenceFragments.
2. `start` and `end` are zero-based UTF-8 byte offsets within that fragment's exact `text`; `end` is exclusive.
3. Every byte of every non-empty fragment must be covered exactly once.
4. Do not rewrite, normalize, paraphrase, summarize or invent source text.
5. Do not output candidate IDs. Plumb derives candidate IDs deterministically.
6. Do not output explanations or Markdown outside the JSON object.
