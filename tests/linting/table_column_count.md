# Table column counts

A matched table is silent.

| a | b |
|---|---|
| 1 | 2 |

The delimiter row below declares two columns, so the third cell of each row
is dropped when the table is rendered.

| a | b | c |
|---|---|
| 1 | 2 | 3 |

A row short of the delimiter's count is padded, not truncated, so nothing is
flagged here.

| a | b |
|---|---|---|
| 1 | 2 | 3 |
