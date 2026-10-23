Warns about YAML values that Quarto and Pandoc read differently. For example,
Pandoc reads `no` as false, while Quarto reads it as the string `"no"`.
Leading-zero values such as `0755` can also have different meanings.

Quote a value if you mean a string. Otherwise, write `true`, `false`, or a
decimal integer explicitly. This check applies to Quarto frontmatter, where both
YAML versions are used.

The optional fix quotes the value. It requires `--unsafe-fixes` because the
author may have intended a boolean or number instead of a string.
