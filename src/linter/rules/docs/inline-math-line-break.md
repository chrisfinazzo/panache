Warns about `\\` at the top level of inline math. It means a row break, so
`\\mathrm{x}` does not mean `\mathrm{x}`.

Remove the extra backslash if you meant a command. If you need several rows, use
display math with a suitable environment. Row breaks inside constructs such as
`matrix`, `array`, and `\substack` are allowed.
