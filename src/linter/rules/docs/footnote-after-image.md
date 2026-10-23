Warns when a footnote immediately follows a standalone image. Pandoc then treats
the image and note as one paragraph, so the image loses its figure caption.
Figure cross-references may also stop resolving.

Move the footnote inside the caption if it belongs there, or add a blank line
between the image and the note to keep them separate. This check applies when
`extensions.implicit-figures` is enabled.
