#!/usr/bin/env node

// PubMed bridge script: reads JSON commands from stdin, calls the
// bibliography-search library, writes JSON results to stdout.
//
// The library path is passed via BIBLIOGRAPHY_SEARCH_DIR env var
// (directory containing dist/index.js).
//
// Protocol:
//   stdin  -> { action: "search" | "detail", params: { ... } }
//   stdout <- { ok: true, data: { ... } }
//   stdout <- { ok: false, error: "..." }

import { stdin, stdout } from "node:process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

// Resolve the library directory from the environment variable.
const libDir = process.env.BIBLIOGRAPHY_SEARCH_DIR;
if (!libDir) {
  writeResponse({ ok: false, error: "BIBLIOGRAPHY_SEARCH_DIR env var is not set" });
  process.exit(1);
}

// Import the library via require() so we can use an absolute path.
const { createPubmedProvider } = require(libDir + "/dist/index.js");

const provider = createPubmedProvider({ backend: "scraping" });

/** Read all of stdin and parse as JSON. */
async function readJson() {
  const chunks = [];
  for await (const chunk of stdin) {
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks).toString("utf8").trim();
  if (!raw) return null;
  return JSON.parse(raw);
}

async function handleSearch(params) {
  return provider.searchByPattern({
    term: params.term,
    sort: params.sort ?? "match",
    sortOrder: params.sortOrder ?? "dsc",
    filter: params.filter ?? [],
    page: params.page ?? 1,
  });
}

async function handleDetail(params) {
  return provider.getArticleDetail(params.pmid);
}

async function main() {
  const input = await readJson();
  if (!input) {
    writeResponse({ ok: false, error: "no input received" });
    return;
  }

  try {
    let data;
    switch (input.action) {
      case "search":
        data = await handleSearch(input.params);
        break;
      case "detail":
        data = await handleDetail(input.params);
        break;
      default:
        writeResponse({ ok: false, error: `unknown action: ${input.action}` });
        return;
    }
    writeResponse({ ok: true, data });
  } catch (err) {
    writeResponse({ ok: false, error: err.message ?? String(err) });
  }
}

function writeResponse(obj) {
  stdout.write(JSON.stringify(obj) + "\n");
}

main().catch((err) => {
  writeResponse({ ok: false, error: err.message ?? String(err) });
  process.exit(1);
});
