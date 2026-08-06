# Display math with long dollar runs

Both delimiters are exactly `$$`; surplus dollars are content or text.

$$
  x^2 + y
$$
$

A longer closing run followed by text on the same line:

$$
  x^2 + y
$$
$ra

A closing run that overshoots by two:

$$
  x^2 +
$$
$$

A longer opening run puts its extra dollar into the content:

$$$x = y$$
$

Four dollars are empty display math, so they stay literal: $$$$
