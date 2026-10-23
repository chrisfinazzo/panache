Warns about links such as `[Methods](#methods)` when the destination ID does not
exist. Correct the link, add an explicit `{#methods}` ID, or link to an existing
heading. IDs are case-sensitive.

The rule recognizes generated heading IDs, explicit attributes, code-cell
labels, and supported HTML IDs. In Quarto and bookdown projects, it also checks
anchors in other project documents and their includes.

Links to another file, full URLs, and bare `#` links are excluded. With
citations enabled, `#ref-key` can resolve to a bibliography entry when the
document cites that key.
