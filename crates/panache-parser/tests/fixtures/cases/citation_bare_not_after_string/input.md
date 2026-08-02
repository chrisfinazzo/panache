# Bare citation notAfterString

A bare `@key` glued to a preceding word character is literal text, not a
citation, matching pandoc's `notAfterString` rule (issue #448).

違法編訂@jzkhl。

An email such as user@example.com stays literal too.

A digit before it, like 1@key, is also suppressed, and so is a trailing dot
in x.@key here.

But non-word punctuation keeps the citation, as in x)@key and _@key here.

The suppress-author form still cites after a word: word-@key remains a
citation because its `@` follows the `-`.

A space before it is the classic in-text form: see @doe99 for details.

A bare key glued to a resolved emphasis or strong closer is suppressed too, so
*em*@key and **strong**@key are literal, but *@key* keeps the citation inside
the emphasis and *em*-@key still cites via the suppress-author form.

Bracketed citations are shielded by the brackets, so word[@doe99] is still a
citation.
