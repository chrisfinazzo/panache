---
base: &defaults
  timeout: 30
prod: &defaults
  timeout: 60
ref: *defaults
---

# Duplicate anchor

The `&defaults` anchor is declared twice in the frontmatter above.
