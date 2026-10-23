Warns when an HTML comment after a heading's attributes prevents Pandoc from
recognizing them. The attributes then appear as text, and links to the intended
heading ID may stop working.

Move the comment to its own line. A heading's `{#id .class}` block must be
followed only by whitespace.
