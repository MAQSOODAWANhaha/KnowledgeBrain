# Bid extraction fixtures

`*.md` files are synthetic, de-identified tender excerpts (`cn-tender-golden-01`–`03`). Their `.expected.json`
files define clause labels and acceptance thresholds.

Office table convert fixture lives in `crates/docparser/src/anydoc.rs` (`docx_table_renders_as_gfm`):
a synthetic `.docx` with a 3-row table must emit GFM pipes. Default engine for `docx` is anydoc.

Offline deterministic check:

```bash
BID_EXTRACT_MODE=heuristic cargo test -p bid golden_fixture
```

Manual real-model evaluation (never part of ordinary CI):

```bash
BID_EXTRACT_MODE=agent \
BID_EXTRACT_MODEL_ID=<tool-capable-model> \
KNOWLEDGEBRAIN_CHAT_BASE_URL=<endpoint> \
KNOWLEDGEBRAIN_CHAT_API_KEY=<key> \
cargo run -p bid --bin bid_extract_eval -- \
  testdata/bid-extraction/cn-tender-golden-01.md \
  /tmp/bid-extract-report.json \
  testdata/bid-extraction/cn-tender-golden-01.expected.json
```

Use a `.md` output path for a human-readable Markdown report with the complete
JSON artifact embedded.

When an expected file is supplied, the evaluator records one-to-one precision,
recall, family/must accuracy, duplicates, unsupported quotes, and false
positives, and exits non-zero if any threshold fails. Assignment requires an
exact normalized label quote (apart from terminal punctuation), or an explicit
`accepted_aliases` entry; short containment fragments do not count. Without an
expected file the report is `NOT_EVALUATED`, never `PASS`. Quotes must always be
continuous substrings of their referenced `quotable_text`; no knowledge-base
access is available to this evaluator.
