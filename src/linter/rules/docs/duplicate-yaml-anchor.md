Warns when a YAML document defines the same `&name` anchor more than once. An
alias may then refer to a different value than you intended.

Rename the duplicate anchor and its aliases, or remove the extra definition.
Anchors in separate frontmatter or code-cell option blocks are independent.
