Warns about link destinations beginning with `@`, such as `[Figure 2](@fig-2)`.
That creates a link to the literal URL `@fig-2`.

Use `@fig-2` on its own for a cross-reference, or `[Figure 2](#fig-2)` for a
link to an ID. The fix changes the destination's leading `@` to `#`.
