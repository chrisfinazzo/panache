Warns when a known citation key touches the preceding word, as in
`See@smith2024`. Pandoc treats that form as ordinary text.

Insert a space (`See @smith2024`) or use a bracketed citation
(`See[@smith2024]`). The rule checks only keys found in a bibliography or inline
`references:` entry, so ordinary email addresses are left alone. Escape a
literal at sign as `\@` if it should not introduce a citation.
