Warns when a link's visible text exactly repeats its destination URL. The fix
replaces `[https://example.com/](https://example.com/)` with the shorter
autolink `<https://example.com/>`.

Links with a title, formatted text, or any difference between the visible URL
and destination are left alone. This includes differences in trailing slashes.
