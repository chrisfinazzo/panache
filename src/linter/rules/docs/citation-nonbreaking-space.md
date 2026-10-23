Warns about a breakable space before a bracketed citation. Line wrapping can
leave the citation at the start of a line, separated from the text it supports.

The fix replaces the gap with Pandoc's nonbreaking space escape, `\ `. If that
escape is disabled, it inserts a nonbreaking space character instead. Citations
at the start of a paragraph, after a hard line break, or already preceded by a
nonbreaking space are left alone.
