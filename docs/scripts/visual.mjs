#!/usr/bin/env bun
/**
 * Visual iteration harness — serves dist/ statically, launches chromium,
 * navigates to a Starlight page, inspects the page structure, and
 * screenshots the full page.
 *
 * Pattern ported from mcp-workflow-and-tech-stack/site/scripts/visual.mjs
 * (which inspects a graph component); here simplified for plain Starlight
 * pages — no graph/canvas, just navigation, error-capture, screenshots.
 *
 * Usage:
 *   bun scripts/visual.mjs                                  # default: / on mobile viewport
 *   bun scripts/visual.mjs /reference/adapter-claude-code/  # specific page
 *   bun scripts/visual.mjs / desktop                        # desktop viewport
 *   bun scripts/visual.mjs / mobile keep                    # keep open after screenshot
 *
 * Outputs to /tmp/pconv-visual-<timestamp>/:
 *   - full.png    Full-page screenshot
 *   - report.md   DOM inspection, console logs, page errors
 *
 * Requires `dist/` to exist — run `bun run build` (or `bun run serve build`)
 * first. Visual is for inspection, not for CI; use smoke.mjs for CI.
 */

import { existsSync, statSync, mkdirSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const __dirname = dirname(fileURLToPath(import.meta.url));
const docsDir = resolve(__dirname, "..");
const distDir = resolve(docsDir, "dist");

const PATH = process.argv[2] || "/";
const VIEWPORT_MODE = process.argv[3] || "mobile";
const KEEP = process.argv.includes("keep");

const PORT = 4323;
const PREFIX = "/portaconv";

const VIEWPORTS = {
  mobile: { width: 390, height: 844 },
  tablet: { width: 768, height: 1024 },
  narrow: { width: 1024, height: 768 },
  desktop: { width: 1440, height: 900 },
};
const viewport = VIEWPORTS[VIEWPORT_MODE] || VIEWPORTS.mobile;

if (!existsSync(distDir)) {
  console.error(`  ERROR: ${distDir} does not exist — run \`bun run build\` first.`);
  process.exit(1);
}

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "application/javascript",
  ".mjs": "application/javascript",
  ".css": "text/css",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".ico": "image/x-icon",
  ".xml": "application/xml",
  ".txt": "text/plain; charset=utf-8",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};
const contentType = (p) =>
  MIME[p.slice(p.lastIndexOf("."))] || "application/octet-stream";

// ── Static server ────────────────────────────────────────────────────
const server = Bun.serve({
  hostname: "127.0.0.1",
  port: PORT,
  fetch(req) {
    const url = new URL(req.url);
    let pathname = decodeURIComponent(url.pathname);
    if (pathname === PREFIX || pathname.startsWith(`${PREFIX}/`)) {
      pathname = pathname.slice(PREFIX.length) || "/";
    }
    let filePath = join(distDir, pathname);
    try {
      if (existsSync(filePath) && statSync(filePath).isDirectory()) {
        filePath = join(filePath, "index.html");
      }
      if (!existsSync(filePath)) {
        if (existsSync(`${filePath}.html`)) filePath = `${filePath}.html`;
        else return new Response("Not found", { status: 404 });
      }
      return new Response(Bun.file(filePath), {
        headers: { "Content-Type": contentType(filePath) },
      });
    } catch (err) {
      return new Response(`Error: ${err.message}`, { status: 500 });
    }
  },
});

console.log(`  Static server on http://127.0.0.1:${PORT}`);
console.log(
  `  Target: ${PATH} @ ${VIEWPORT_MODE} (${viewport.width}x${viewport.height})`,
);

const outDir = `/tmp/pconv-visual-${Date.now()}`;
mkdirSync(outDir, { recursive: true });
console.log(`  Output: ${outDir}`);

// ── Browser + inspect ────────────────────────────────────────────────
const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({ viewport });
const page = await context.newPage();

const consoleLogs = [];
const pageErrors = [];
page.on("console", (msg) => {
  consoleLogs.push({ type: msg.type(), text: msg.text() });
});
page.on("pageerror", (err) => pageErrors.push(err.message));

const url = `http://127.0.0.1:${PORT}${PREFIX}${PATH}`;
console.log(`  Navigating to ${url}`);
const response = await page.goto(url, {
  waitUntil: "networkidle",
  timeout: 30_000,
});
const httpStatus = response ? response.status() : 0;
console.log(`  HTTP ${httpStatus}`);

// ── Starlight page inspection ────────────────────────────────────────
const inspection = await page.evaluate(() => {
  const result = {};

  result.title = document.title;
  result.metaDescription =
    document.querySelector('meta[name="description"]')?.getAttribute("content") ||
    null;

  const header = document.querySelector("header.header");
  result.header = {
    found: !!header,
    siteTitle:
      document.querySelector('a[href][rel="home"]')?.textContent?.trim() ||
      document.querySelector(".site-title")?.textContent?.trim() ||
      null,
  };

  const sidebar = document.querySelector("nav.sidebar, .sidebar-content");
  result.sidebar = {
    found: !!sidebar,
    groupCount: document.querySelectorAll(".sidebar nav ul > li, .large").length,
    linkCount: document.querySelectorAll(".sidebar a[href]").length,
  };

  const main = document.querySelector("main");
  result.main = {
    found: !!main,
    h1: main?.querySelector("h1")?.textContent?.trim() || null,
    h2Count: main?.querySelectorAll("h2").length || 0,
    h3Count: main?.querySelectorAll("h3").length || 0,
    tableCount: main?.querySelectorAll("table").length || 0,
    codeBlockCount: main?.querySelectorAll("pre > code").length || 0,
    textLength: main?.textContent?.trim().length || 0,
  };

  // Flexoki theme loaded = CSS var --sl-color-accent is present.
  const root = getComputedStyle(document.documentElement);
  result.theme = {
    slColorAccent: root.getPropertyValue("--sl-color-accent").trim(),
    slColorBg: root.getPropertyValue("--sl-color-bg").trim(),
  };

  // Pagefind search wiring.
  result.pagefind = {
    searchEl: !!document.querySelector(
      '[data-pagefind-search], #starlight__search, [data-pagefind-ui]',
    ),
  };

  // Broken-image detection — any <img> that failed to load.
  const broken = Array.from(document.images)
    .filter((img) => img.complete && img.naturalWidth === 0)
    .map((img) => img.src);
  result.brokenImages = broken;

  return result;
});

console.log(`\n  === Inspection ===`);
console.log(JSON.stringify(inspection, null, 2));

// ── Screenshots ──────────────────────────────────────────────────────
await page.screenshot({ path: join(outDir, "full.png"), fullPage: true });
console.log(`  Saved: ${outDir}/full.png`);

// ── Report ───────────────────────────────────────────────────────────
const errorBlock = [
  `## Page errors`,
  ...(pageErrors.length ? pageErrors.map((e) => `- ${e}`) : ["(none)"]),
  ``,
  `## Console logs`,
  `Total: ${consoleLogs.length}`,
  ...consoleLogs.slice(0, 30).map((l) => `- [${l.type}] ${l.text}`),
];

const report = [
  `# pconv visual report`,
  ``,
  `- URL: ${url}`,
  `- HTTP status: ${httpStatus}`,
  `- Viewport: ${VIEWPORT_MODE} (${viewport.width}x${viewport.height})`,
  `- Timestamp: ${new Date().toISOString()}`,
  ``,
  `## Page metadata`,
  `- title: ${inspection.title}`,
  `- description: ${inspection.metaDescription}`,
  ``,
  `## Starlight structure`,
  `\`\`\`json`,
  JSON.stringify(
    { header: inspection.header, sidebar: inspection.sidebar, main: inspection.main },
    null,
    2,
  ),
  `\`\`\``,
  ``,
  `## Theme tokens`,
  `\`\`\`json`,
  JSON.stringify(inspection.theme, null, 2),
  `\`\`\``,
  ``,
  `## Pagefind search`,
  `- search element present: ${inspection.pagefind.searchEl}`,
  ``,
  `## Broken images`,
  inspection.brokenImages.length
    ? inspection.brokenImages.map((s) => `- ${s}`).join("\n")
    : `(none)`,
  ``,
  ...errorBlock,
].join("\n");

await writeFile(join(outDir, "report.md"), report);
console.log(`  Saved: ${outDir}/report.md`);

// Any page error is a hard fail — makes this usable as a quick gate.
let exitCode = 0;
if (httpStatus !== 200) {
  console.error(`\n  FAIL  HTTP ${httpStatus} (expected 200)`);
  exitCode = 1;
}
if (pageErrors.length > 0) {
  console.error(`\n  FAIL  ${pageErrors.length} page error(s)`);
  exitCode = 1;
}
if (!inspection.main.found) {
  console.error(`\n  FAIL  <main> element not found`);
  exitCode = 1;
}
if (inspection.brokenImages.length > 0) {
  console.error(`\n  FAIL  ${inspection.brokenImages.length} broken image(s)`);
  exitCode = 1;
}

// ── Teardown ─────────────────────────────────────────────────────────
if (!KEEP) {
  await browser.close();
  server.stop(true);
  console.log(exitCode === 0 ? `\n  OK` : `\n  FAILED`);
  process.exit(exitCode);
} else {
  console.log(`\n  (keeping browser open; Ctrl+C to exit)`);
  await new Promise(() => {});
}
