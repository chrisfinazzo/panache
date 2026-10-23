Reports YAML frontmatter keys that Pandoc cannot use as metadata. Collection
keys such as `[a, b]: value` and alias keys such as `*name: value` can prevent
the document from rendering.

Replace the key with a normal name, or move the content out of the frontmatter
if it was not meant to be metadata. Scalar keys such as `1` are accepted.
Code-cell options are not checked by this rule.
