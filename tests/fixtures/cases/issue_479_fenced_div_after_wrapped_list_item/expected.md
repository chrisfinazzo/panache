- When determining the target file, they can deal with the presence or absence
  of the `.R` extension and the `test-` prefix.
- If the target file already exists, it is opened for editing. Otherwise, the
  target is created and then opened for editing.

::: callout-tip
## RStudio

If `R/foofy.R` is the active file in your source editor, you can call
`use_test()` with no arguments.
:::
