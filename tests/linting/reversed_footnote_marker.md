# Reversed footnote markers

A note written backwards [^this should have been an inline note] here.

A backwards note spanning [^lines, which pandoc renders as literal
text instead] here.

A backwards note carrying [^a citation key like
(@doe2026), which pandoc reads as a citation] here.

A bare label is a real footnote reference[^ok] here.

An inline footnote uses the right marker^[this one is fine] here.

A link whose text starts with a caret is left alone: [^multi
line](https://example.com).

A swap that would build a link instead is reported without a fix:
[^this note](https://example.com).

[^ok]: A defined note.
