Warns when `[` or `(` directly follows an inline footnote's closing bracket.
Pandoc may read the note as a link label instead, leaving the footnote absent
from the output.

The fix inserts a space after the note. For the `(` form it requires
`--unsafe-fixes`: confirm that you intended a footnote rather than a link
preceded by a literal caret.
