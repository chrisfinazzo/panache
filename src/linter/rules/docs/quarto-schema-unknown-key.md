Warns about unknown or misspelled Quarto options in frontmatter, code-cell
options, and project metadata. Check the suggested spelling or remove the unused
option.

This rule is optional because Quarto allows custom metadata. At the top level,
it warns only about names close to known options. Within sections that permit
only predefined options, it reports any unknown key.

Like [`quarto-schema`](#quarto-schema), it honors `validate-yaml: false`.
