Warns about YAML values that do not match Quarto's expected type or allowed
choices. It checks frontmatter, code-cell options, and project files such as
`_quarto.yml` and `_metadata.yml`.

Use the expected type or one of the suggested values. Unknown keys are handled
separately by [`quarto-schema-unknown-key`](#quarto-schema-unknown-key).

Set `validate-yaml: false` in the affected document or manifest to skip schema
checks when a filter needs a different form. YAML syntax errors are still
reported. Some deeply nested options and restrictions tied to a particular
output format are not checked.
