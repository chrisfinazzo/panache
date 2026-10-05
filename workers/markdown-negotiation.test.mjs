import assert from "node:assert/strict";
import test from "node:test";

import { handleRequest } from "./markdown-negotiation.mjs";

function origin(requests, markdownStatus = 200) {
  return async (request) => {
    requests.push(new URL(request.url).pathname);
    if (request.url.endsWith(".llms.md")) {
      return new Response("# Page\n", { status: markdownStatus });
    }
    return new Response("<h1>Page</h1>", {
      headers: { "Content-Type": "text/html", Vary: "Accept-Encoding" },
    });
  };
}

test("serves HTML by default", async () => {
  const requests = [];
  const response = await handleRequest(
    new Request("https://panache.bz/guide/formatting.html"),
    origin(requests),
  );
  assert.deepEqual(requests, ["/guide/formatting.html"]);
  assert.equal(response.headers.get("Content-Type"), "text/html");
  assert.equal(response.headers.get("Vary"), "Accept-Encoding, Accept");
});

test("serves generated Markdown at the original page URL", async () => {
  const requests = [];
  const response = await handleRequest(
    new Request("https://panache.bz/guide/formatting.html", {
      headers: { Accept: "text/markdown" },
    }),
    origin(requests),
  );
  assert.deepEqual(requests, ["/guide/formatting.llms.md"]);
  assert.equal(response.headers.get("Content-Type"), "text/markdown; charset=utf-8");
  assert.equal(response.headers.get("Vary"), "Accept");
  assert.equal(await response.text(), "# Page\n");
});

test("maps the homepage and directory URLs to index Markdown", async () => {
  for (const [page, markdown] of [
    ["/", "/index.llms.md"],
    ["/guide/", "/guide/index.llms.md"],
  ]) {
    const requests = [];
    await handleRequest(
      new Request(`https://panache.bz${page}`, {
        headers: { Accept: "text/markdown" },
      }),
      origin(requests),
    );
    assert.deepEqual(requests, [markdown]);
  }
});

test("falls back to the original page when Markdown is unavailable", async () => {
  const requests = [];
  const response = await handleRequest(
    new Request("https://panache.bz/guide/missing.html", {
      headers: { Accept: "text/markdown" },
    }),
    origin(requests, 404),
  );
  assert.deepEqual(requests, ["/guide/missing.llms.md", "/guide/missing.html"]);
  assert.equal(response.headers.get("Content-Type"), "text/html");
  assert.equal(response.headers.get("Vary"), "Accept-Encoding, Accept");
});

test("ignores rejected Markdown and non-page requests", async () => {
  for (const [path, accept] of [
    ["/guide/lsp.html", "text/markdown;q=0, text/html"],
    ["/images/logo.svg", "text/markdown"],
    ["/playground/", "text/markdown"],
    ["/llms.txt", "text/markdown"],
  ]) {
    const requests = [];
    await handleRequest(
      new Request(`https://panache.bz${path}`, { headers: { Accept: accept } }),
      origin(requests),
    );
    assert.deepEqual(requests, [path]);
  }
});
