Warns about reference links, cross-references, and footnotes whose labels have
no definition. Add the missing definition or correct the label. This includes
shortcut links such as `[guide]` and collapsed links such as `[guide][]`.

Cross-references with configured `crossref-prefixes` receive the same target
validation as built-in prefixes. Targets must have a matching ID or executable
chunk label in the document or project. Panache does not execute code or
extensions to discover generated targets.

With citations enabled, a narrative citation's suffix, such as `[p. 4]` in
`@author [p. 4]`, belongs to the citation and needs no reference definition. The
suffix can follow the citation key on the next line in the same paragraph.

Labels include their inline markup: `[<span>Guide</span>]` does not match a
definition named `[Guide]`.
