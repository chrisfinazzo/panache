# Markdown negotiation

This Worker serves Quarto's generated `.llms.md` file when an agent requests
`text/markdown` at the corresponding page URL. The HTML site remains on GitHub
Pages. Requests for assets and the playground pass through to GitHub Pages.

Run the focused tests before deployment:

```sh
node --test workers/markdown-negotiation.test.mjs
wrangler deploy --dry-run --config workers/wrangler.jsonc
```

The `panache.bz` DNS record must remain proxied through Cloudflare. After
authenticating with `wrangler login`, deploy the Worker and its route:

```sh
wrangler deploy --config workers/wrangler.jsonc
```

In the Cloudflare dashboard, set the `panache.bz/*` Worker route to **Fail
open**. This keeps the GitHub Pages site available if the Workers Free daily
request limit is reached.

Check both representations on the live site:

```sh
curl -i https://panache.bz/getting-started.html -H 'Accept: text/markdown'
curl -i https://panache.bz/getting-started.html
```

The first response should have `Content-Type: text/markdown; charset=utf-8` and
`Vary: Accept`; the second should remain HTML.
