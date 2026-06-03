#!/usr/bin/env node
// Build the publishable npm package into ./pkg:
//   1. wasm-pack compiles the crate (ESM glue + .wasm + .d.ts + package.json)
//   2. bundle the stylesheet, inlining the toolbar sprite as a data URI so
//      consumers need no static assets (the Trunk demo keeps using /asset/)
//   3. patch package.json (expose the CSS, keywords, sideEffects) and copy
//      LICENSE / a link-fixed README for the npmjs.com page
//
// Usage:  node scripts/build-npm.mjs   →   cd pkg && npm publish

import { execFileSync } from "node:child_process";
import { copyFileSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const REPO_URL = "https://github.com/xiujunma/zedsheet";
const KEYWORDS = ["spreadsheet", "wasm", "webassembly", "canvas", "excel"];

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const pkgDir = path.join(root, "pkg");
const out = (msg) => process.stdout.write(msg + "\n");

// ---------------------------------------------------------------------------
// 1. Compile. --target web = ESM that fetches its own .wasm via init().
// ---------------------------------------------------------------------------
out("• wasm-pack build (release, target web)…");
execFileSync(
  "wasm-pack",
  ["build", "--release", "--target", "web", "--out-dir", "pkg", "--out-name", "zedsheet"],
  { cwd: root, stdio: "inherit" },
);

// ---------------------------------------------------------------------------
// 2. Self-contained stylesheet: src/index.css with the sprite inlined.
// ---------------------------------------------------------------------------
const SPRITE_REF = "url('/asset/sprite.svg')";
const css = readFileSync(path.join(root, "src", "index.css"), "utf8");
if (!css.includes(SPRITE_REF)) {
  throw new Error(
    `src/index.css no longer references ${SPRITE_REF} — update scripts/build-npm.mjs to match`,
  );
}
const sprite = readFileSync(path.join(root, "asset", "sprite.svg"));
const dataUri = `url('data:image/svg+xml;base64,${sprite.toString("base64")}')`;
writeFileSync(path.join(pkgDir, "zedsheet.css"), css.replaceAll(SPRITE_REF, dataUri));
out("• pkg/zedsheet.css written (sprite inlined)");

// ---------------------------------------------------------------------------
// 3. Patch the generated package.json and copy docs.
// ---------------------------------------------------------------------------
const manifestPath = path.join(pkgDir, "package.json");
const generated = JSON.parse(readFileSync(manifestPath, "utf8"));
const manifest = {
  ...generated,
  files: [...new Set([...(generated.files ?? []), "zedsheet.css"])],
  exports: {
    ".": { types: "./zedsheet.d.ts", default: "./zedsheet.js" },
    "./zedsheet.css": "./zedsheet.css",
    "./zedsheet_bg.wasm": "./zedsheet_bg.wasm",
  },
  style: "./zedsheet.css",
  sideEffects: [...new Set([...(generated.sideEffects ?? []), "./zedsheet.css"])],
  keywords: KEYWORDS,
  homepage: REPO_URL,
};
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");

// npmjs.com renders the package README; make repo-relative links absolute.
// Images must point at raw.githubusercontent.com (blob/ URLs serve an HTML
// page, not image bytes) — rewrite them FIRST, then the remaining doc links.
const RAW_URL = REPO_URL.replace("github.com", "raw.githubusercontent.com");
const readme = readFileSync(path.join(root, "README.md"), "utf8")
  .replace(/(!\[[^\]]*\]\()\.\/([^)]+?\.(?:png|jpe?g|gif|svg))\)/g, `$1${RAW_URL}/main/$2)`)
  .replaceAll("](./", `](${REPO_URL}/blob/main/`);
writeFileSync(path.join(pkgDir, "README.md"), readme);
copyFileSync(path.join(root, "LICENSE"), path.join(pkgDir, "LICENSE"));

out(`• pkg/package.json patched — ${manifest.name}@${manifest.version}`);
out("Done. Publish with:  cd pkg && npm publish");
