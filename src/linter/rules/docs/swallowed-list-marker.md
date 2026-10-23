Warns when a line that looks like a list item is read as part of the preceding
paragraph. Add a blank line before the list. If the marker is ordinary prose,
escape it, as in `\-`.

Pandoc requires the blank line before a list. CommonMark and GFM allow bullets
and lists starting at `1` to interrupt a paragraph, but a list starting at `2`
still needs separation. The rule reports the first line in a consecutive run of
affected items.
