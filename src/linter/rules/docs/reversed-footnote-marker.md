Warns about `[^...]` containing prose where an inline footnote may have been
intended. Inline footnotes start with `^[`, as in `^[A note]`.

Confirm the intended meaning before applying the fix, which requires
`--unsafe-fixes`. A valid but undefined label such as `[^missing]` belongs to
[`undefined-references`](#undefined-references). Working links and cases where
swapping the marker would still fail to create a footnote are left unfixed.
