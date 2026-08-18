---
paths:
  - "src/formatter.rs"
  - "crates/panache-formatter/**/*.rs"
  - "crates/panache-formatter/tests/format/**/*.rs"
  - "tests/fixtures/cases/**/expected.md"
---

Formatter changes should preserve determinism and keep parser policy separate.

- Enforce idempotency: `format(format(x)) == format(x)`.
- If idempotency fails, check parser CST shape before adding formatter
  workarounds.
- Reuse existing helpers (wrapping/lists/tables) instead of duplicating logic.
- Keep formatter core logic in `crates/panache-formatter`; keep host runtime and
  process integration in top-level `src/`.
- Add or update the smallest relevant formatter golden case for user-visible
  behavior changes.
- Update formatting docs for user-visible rule changes.
