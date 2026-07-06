import { createReadStream, existsSync } from "node:fs";
import { stat } from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import { pathToFileURL } from "node:url";

export function assert(condition, message) {
  if (!condition) {
    throw new Error(`ASSERT FAILED: ${message}`);
  }
}

// Exported: the e2e-oblivious suite reuses this Chromium loader verbatim.
export async function loadChromium() {
  const corePath = process.env.PLAYWRIGHT_CORE;
  assert(corePath && existsSync(corePath), `PLAYWRIGHT_CORE must point at playwright-core (got: ${corePath})`);
  const { chromium } = await import(pathToFileURL(corePath).href);
  return chromium;
}

function contentType(filePath) {
  if (filePath.endsWith(".html")) return "text/html; charset=utf-8";
  if (filePath.endsWith(".js")) return "text/javascript; charset=utf-8";
  if (filePath.endsWith(".css")) return "text/css; charset=utf-8";
  return "application/octet-stream";
}

// Exported: the e2e-oblivious suite reuses this static server verbatim.
export async function startStaticServer(root) {
  const server = http.createServer(async (request, response) => {
    try {
      const url = new URL(request.url || "/", "http://127.0.0.1");
      const requested = url.pathname === "/" ? "/index.html" : decodeURIComponent(url.pathname);
      const filePath = path.resolve(root, `.${requested}`);
      if (!filePath.startsWith(`${root}${path.sep}`) && filePath !== root) {
        response.writeHead(403);
        response.end("forbidden");
        return;
      }
      const info = await stat(filePath);
      if (!info.isFile()) {
        response.writeHead(404);
        response.end("not found");
        return;
      }
      response.writeHead(200, { "content-type": contentType(filePath) });
      createReadStream(filePath).pipe(response);
    } catch {
      response.writeHead(404);
      response.end("not found");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert(address && typeof address === "object", "static server did not expose a TCP address");
  return {
    origin: `http://127.0.0.1:${address.port}`,
    close: () => new Promise((resolve, reject) => server.close((error) => (error ? reject(error) : resolve()))),
  };
}

export async function withDemoGui(body) {
  const chromium = await loadChromium();
  const pagePath = process.env.DEMO_GUI_HTML;
  const chromiumBin = process.env.CHROMIUM_BIN;
  assert(pagePath && existsSync(pagePath), `DEMO_GUI_HTML must point at index.html (got: ${pagePath})`);
  assert(chromiumBin && existsSync(chromiumBin), `CHROMIUM_BIN must point at store Chromium (got: ${chromiumBin})`);
  const root = path.dirname(path.resolve(pagePath));
  const server = await startStaticServer(root);

  let browser = null;
  try {
    browser = await chromium.launch({
      executablePath: chromiumBin,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    });
    const page = await browser.newPage({ viewport: { width: 1440, height: 1200 } });
    const pageErrors = [];
    const consoleMessages = [];
    page.on("pageerror", (error) => pageErrors.push(String(error)));
    page.on("console", (message) => consoleMessages.push(`${message.type()}: ${message.text()}`));
    await page.goto(`${server.origin}/${path.basename(pagePath)}`, { waitUntil: "load" });
    await body(page, { consoleMessages, pageErrors });
  } finally {
    if (browser) await browser.close();
    await server.close();
  }
}
