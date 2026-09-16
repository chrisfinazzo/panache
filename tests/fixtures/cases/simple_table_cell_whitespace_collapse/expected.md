  Head1   Head2
  ------- -------
  A B     c d

Code spans keep their runs:

  Head1        Head2
  ------------ -------
  A `x  y` B   c d

Widest cell has runs:

  A B
  ----- --
  x y

Nested prose keeps literal content intact:

  Head1                       Head2
  --------------------------- -------
  *I will **do something***   x
  **a `b  c` d**              y
