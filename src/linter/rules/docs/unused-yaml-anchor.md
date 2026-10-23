Warns about a YAML `&name` anchor with no matching `*name` alias in the same
YAML document. Remove the anchor if it is no longer needed, or use an alias
where you meant to reuse the value.

Aliases in another frontmatter or code-cell option block do not count as uses of
the anchor.
