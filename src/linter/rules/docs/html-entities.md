Warns about misspelled HTML named entities and missing semicolons, such as
`&ellips;` or `&numero`. These may appear literally in rendered text instead of
producing the intended character.

Correct the name and end it with a semicolon, or insert the character directly.
The rule checks prose and skips code, math, raw HTML, and numeric references
such as `&#123;`.
