#!/usr/bin/env node
// Publish the built pkg/ to GitHub Packages as @xiujunma/zedsheet so it shows
// under https://github.com/xiujunma?tab=packages and on the repo sidebar.
//
// This is a SHOWCASE copy: GitHub's npm registry requires a scoped name and
// authenticated installs, so npmjs.org (unscoped `zedsheet`) remains the
// canonical install source. The `repository` field links the package to the
// repo, which makes it public (inheriting the repo's visibility).
//
// Auth: a GitHub token with `write:packages` — taken from $GITHUB_TOKEN or
// `gh auth token` (grant with: gh auth refresh -s read:packages,write:packages)
//
// Usage:  node scripts/build-npm.mjs && node scripts/publish-github.mjs

import { execFileSync } from "node:child_process";
import { cpSync, existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const SCOPED_NAME = "@xiujunma/zedsheet";
const REGISTRY = "https://npm.pkg.github.com";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const pkgDir = path.join(root, "pkg");
const outDir = path.join(root, "pkg-github");
const out = (msg) => process.stdout.write(msg + "\n");

if (!existsSync(path.join(pkgDir, "package.json"))) {
  throw new Error("pkg/ not built — run `node scripts/build-npm.mjs` first");
}

const token =
  process.env.GITHUB_TOKEN ??
  execFileSync("gh", ["auth", "token"]).toString().trim();
if (!token) {
  throw new Error(
    "no GitHub token — set $GITHUB_TOKEN or `gh auth refresh -s read:packages,write:packages`",
  );
}

// Fresh scoped copy of the built package (never mutate pkg/ itself).
rmSync(outDir, { recursive: true, force: true });
cpSync(pkgDir, outDir, { recursive: true });

const manifestPath = path.join(outDir, "package.json");
const generated = JSON.parse(readFileSync(manifestPath, "utf8"));
const manifest = {
  ...generated,
  name: SCOPED_NAME,
  publishConfig: { registry: REGISTRY },
};
writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");

// Token via env-var reference so it never lands on disk (.npmrc is also on
// npm's always-excluded list, so it can't leak into the tarball).
writeFileSync(
  path.join(outDir, ".npmrc"),
  `//npm.pkg.github.com/:_authToken=\${NODE_AUTH_TOKEN}\n`,
);

out(`• publishing ${SCOPED_NAME}@${manifest.version} → ${REGISTRY}`);
execFileSync("npm", ["publish"], {
  cwd: outDir,
  stdio: "inherit",
  env: { ...process.env, NODE_AUTH_TOKEN: token },
});

rmSync(outDir, { recursive: true, force: true });
out("Done: https://github.com/xiujunma/zedsheet/pkgs/npm/zedsheet");
