Warns when a line that looks like a list item is read as part of the preceding
paragraph. Add a blank line before the list. If the marker is ordinary prose,
escape it, as in `\-`.

Pandoc requires the blank line before a list. CommonMark and GFM allow bullets
and lists starting at `1` to interrupt a paragraph, but a list starting at `2`
still needs separation. The rule reports the first line in a consecutive run of
affected items.

A paragraph with just one possible numbered marker, such as `10.`, stays quiet
when the preceding line ends in a word or number and the text after the marker
starts with a capital letter. This can be a number at the end of a wrapped
sentence, as in `increases by a factor` followed by `10. The other runtimes`.
Single items after a colon or other punctuation still warn, as do paragraphs
with multiple marker lines. Bullets and markers ending in `)` also still warn.
