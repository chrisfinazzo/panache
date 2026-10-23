Warns about YAML keys with nothing after the colon, such as `title:`. YAML
treats these values as null, which may indicate an unfinished setting.

Supply a value, remove the key, or write `null` or `~` if null is intentional.
The rule checks frontmatter and code-cell options, but not mappings written
inside braces, such as `{title: }`.

The optional fix removes the whole key. It requires `--unsafe-fixes` because
removing a setting can change the document's behavior.
