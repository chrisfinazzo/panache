Warns when a pipe table has more cells in a row than its separator row declares.
Pandoc drops the extra cells when rendering the table.

Add the missing columns to the separator row, or remove the extra cells if they
are unintended. Shorter rows are allowed because they are padded with empty
cells.
