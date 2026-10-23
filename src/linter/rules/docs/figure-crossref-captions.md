Warns when a bookdown figure reference, such as `\@ref(fig:plot)`, targets a
code cell without a figure caption. A label alone does not create the figure
that the reference needs.

Add a `fig.cap` or `fig-cap` option to the target cell, or correct the reference
if it points to the wrong cell.
