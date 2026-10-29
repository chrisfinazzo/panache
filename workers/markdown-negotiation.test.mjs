import assert from "node:assert/strict";
import test from "node:test";

import { handleRequest } from "./markdown-negotiation.mjs";

test("redirects legacy documentation and directory aliases", async () => {
  for (const [path, target] of [
    ["/user-guide", "/guide/"],
    ["/user-guide/", "/guide/"],
    ["/user-guide/index.html", "/guide/"],
    ["/user-guide/configuration", "/reference/configuration.html"],
    ["/user-guide/configuration.html", "/reference/configuration.html"],
    ["/user-guide/integrations", "/guide/integrations.html"],
    ["/user-guide/integrations.html", "/guide/integrations.html"],
    ["/user-guide/lsp", "/guide/lsp.html"],
    ["/user-guide/lsp.html", "/guide/lsp.html"],
    ["/user-guide/formatting", "/guide/formatting.html"],
    ["/user-guide/formatting.html", "/guide/formatting.html"],
    ["/guide/configuration", "/reference/configuration.html"],
    ["/guide/configuration.html", "/reference/configuration.html"],
    ["/index.html", "/"],
    ["/guide/index.html", "/guide/"],
    ["/reference/index.html", "/reference/"],
    ["/development/index.html", "/development/"],
    ["/playground", "/playground/"],
    ["/playground/index.html", "/playground/"],
  ]) {
    const response = await handleRequest(
      new Request(`https://panache.bz${path}`),
      () => assert.fail("A redirect must not fetch the origin."),
    );
    assert.equal(response.status, 301, path);
    assert.equal(
      response.headers.get("Location"),
      `https://panache.bz${target}`,
      path,
    );
  }
});

test("redirects preserve query strings and run before Markdown negotiation", async () => {
  for (const method of ["GET", "HEAD"]) {
    const response = await handleRequest(
      new Request("http://panache.bz/playground/index.html?flavor=quarto&lw=88", {
        method,
        headers: { Accept: "text/markdown" },
      }),
      () => assert.fail("A redirect must not fetch the origin."),
    );
    assert.equal(response.status, 301);
    assert.equal(
      response.headers.get("Location"),
      "https://panache.bz/playground/?flavor=quarto&lw=88",
    );
  }
});

test("unknown legacy pages and non-read requests reach the origin", async () => {
  for (const [path, method] of [
    ["/user-guide/missing.html", "GET"],
    ["/missing/index.html", "GET"],
    ["/user-guide/configuration.html", "POST"],
  ]) {
    const requests = [];
    const response = await handleRequest(
      new Request(`https://panache.bz${path}`, { method }),
      async (request) => {
        requests.push([new URL(request.url).pathname, request.method]);
        return new Response("Not found", { status: 404 });
      },
    );
    assert.equal(response.status, 404);
    assert.deepEqual(requests, [[path, method]]);
  }
});

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
