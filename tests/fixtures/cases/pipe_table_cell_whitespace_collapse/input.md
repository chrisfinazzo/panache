| a   b | c   d |
|-------|:-----:|
| x   y | `z  w` |

Runs around inline elements collapse to one space:

| Head      |
|-----------|
| a  *b*  c |

Nested prose keeps literal content intact:

| *Head   text* |
| --- |
| *I   will   **do   something*** |
| **outer   *inner   words*** |
| [a   **b   c**](u "a  title") |
| ![a   b](image.png "a  title") |
| ~~a   **b   c**~~ |
| [a   `b  c`]{.note} |
| *a   `b  c`   d* |
| *a\  b   c* |
| *a  b   c* |
| *a   <i title="b  c">d</i>* |
| *a   `b  c`{=html}* |
| *a		b* |
| *a   ^[b   `c  d`]* |
