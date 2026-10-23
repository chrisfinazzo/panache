Warns about an HTML attribute value with an opening quote but no closing quote.
A tag such as `<a id="goal></a>` may appear as ordinary text instead of creating
a link or anchor.

Close the quote at the intended end of the value. Multiline values and unquoted
attributes are allowed. The rule skips code, math, comments, and explicit raw
HTML code spans or fences.
