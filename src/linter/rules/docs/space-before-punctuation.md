Warns about spaces or tabs before `.`, `,`, and `;` in paragraph text. The
optional fix removes the gap.

Check the language and intended spacing before applying the fix with
`--unsafe-fixes`. French documents retain spaces before semicolons. The
document's YAML `lang` setting takes precedence over `[format].lang`.

The rule skips code, math, headings, link text, and punctuation sequences such
as `. . .`. The formatter preserves these spaces unless you apply a lint fix.
