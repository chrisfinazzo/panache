function acceptsMarkdown(header) {
  return header.split(",").some((part) => {
    const [type, ...parameters] = part.trim().toLowerCase().split(";");
    if (type.trim() !== "text/markdown") return false;
    const quality = parameters
      .map((parameter) => parameter.trim().match(/^q\s*=\s*([0-9.]+)$/))
      .find(Boolean);
    return !quality || (Number(quality[1]) > 0 && Number(quality[1]) <= 1);
  });
}

function markdownPath(path) {
  if (path === "/playground" || path.startsWith("/playground/")) return null;
  if (path.endsWith("/")) return `${path}index.llms.md`;
  if (path.endsWith(".html")) return `${path.slice(0, -5)}.llms.md`;
  if (!path.slice(path.lastIndexOf("/") + 1).includes(".")) return `${path}.llms.md`;
  return null;
}

function varyOnAccept(response, markdown = false) {
  const headers = new Headers(response.headers);
  if (markdown) headers.set("Content-Type", "text/markdown; charset=utf-8");
  const vary = headers.get("Vary");
  if (vary !== "*" && !vary?.split(",").some((name) => name.trim().toLowerCase() === "accept")) {
    headers.set("Vary", vary ? `${vary}, Accept` : "Accept");
  }
  return new Response(response.body, { status: response.status, headers });
}

export async function handleRequest(request, fetchOrigin = fetch) {
  const url = new URL(request.url);
  const path = markdownPath(url.pathname);
  if (!path || (request.method !== "GET" && request.method !== "HEAD")) {
    return fetchOrigin(request);
  }

  if (acceptsMarkdown(request.headers.get("Accept") ?? "")) {
    const markdownUrl = new URL(request.url);
    markdownUrl.pathname = path;
    markdownUrl.search = "";
    const headers = new Headers(request.headers);
    headers.delete("If-None-Match");
    headers.delete("If-Modified-Since");
    headers.delete("Range");
    const markdown = await fetchOrigin(new Request(markdownUrl, { method: request.method, headers }));
    if (markdown.ok) return varyOnAccept(markdown, true);
  }

  return varyOnAccept(await fetchOrigin(request));
}

export default {
  fetch(request, _env, context) {
    context.passThroughOnException();
    return handleRequest(request);
  },
};
