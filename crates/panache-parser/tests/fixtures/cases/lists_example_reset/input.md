(@before) First example.
(10@chapter)   Reset to ten with enough text to wrap across multiple lines while preserving its marker.
(@after) Next example.

Some text separates the lists.

(1@)   Reset without a label.
(@tail) After the unlabeled reset.

- Outer item.

  (1@nested)   Nested reset.
  (12@nested-next) Another reset in the same nested list.
  (@nested-last) Continue numbering.

The quoted examples follow.

> (3@quoted)   A quoted reset with enough text to wrap across multiple lines.
> (@quoted-next) Continue in the quote.

A paragraph has enough prose to wrap before (1@literal) stays within the paragraph.
