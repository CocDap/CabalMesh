// Single source of truth for the app version: the root package.json.
//
// Copies its "version" into every other file that carries the app version,
// so a release bump is one command:
//
//   npm version 0.2.0 --no-git-tag-version
//
// npm bumps package.json + package-lock.json, then its "version" lifecycle
// hook runs this script to update the rest.
//
//   node scripts/sync-version.ts           write the version everywhere
//   node scripts/sync-version.ts --check   exit 1 if any file has drifted
//
// Android versionCode is deliberately not touched: it is a monotonically
// increasing integer that CI sets per build, not a semver string.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = process.env.SYNC_VERSION_ROOT ?? join(dirname(fileURLToPath(import.meta.url)), "..");

type Rule = {
  /** Matches the version with the text around it in groups 1 and 2. */
  pattern: RegExp;
  /** How many matches the file must contain; guards against silent pattern rot. */
  count: number;
};

type Target = { path: string; rules: Rule[] };

const jsonTopLevel: Rule = { pattern: /^(\s{2}"version":\s*")[^"]*(")/m, count: 1 };
const lockRoot: Rule = { pattern: /("packages":\s*\{\s*"":\s*\{[^{}]*?"version":\s*")[^"]*(")/, count: 1 };

const targets: Target[] = [
  { path: "package-lock.json", rules: [jsonTopLevel, lockRoot] },
  { path: "web/package.json", rules: [jsonTopLevel] },
  { path: "web/package-lock.json", rules: [jsonTopLevel, lockRoot] },
  { path: "src-tauri/tauri.conf.json", rules: [jsonTopLevel] },
  { path: "src-tauri/tauri.node2.conf.json", rules: [jsonTopLevel] },
  {
    path: "src-tauri/Cargo.toml",
    rules: [{ pattern: /(\[package\][^[]*?\nversion\s*=\s*")[^"]*(")/, count: 1 }],
  },
  {
    path: "src-tauri/Cargo.lock",
    rules: [{ pattern: /(\[\[package\]\]\nname = "cabalmesh"\nversion = ")[^"]*(")/, count: 1 }],
  },
  {
    path: "src-tauri/gen/apple/project.yml",
    rules: [
      { pattern: /(CFBundleShortVersionString:\s*"?)[^"\n]*("?\n)/, count: 1 },
      { pattern: /(CFBundleVersion:\s*"?)[^"\n]*("?\n)/, count: 1 },
    ],
  },
  {
    path: "src-tauri/gen/apple/cabalmesh_iOS/Info.plist",
    rules: [
      { pattern: /(<key>CFBundleShortVersionString<\/key>\s*<string>)[^<]*(<\/string>)/, count: 1 },
      { pattern: /(<key>CFBundleVersion<\/key>\s*<string>)[^<]*(<\/string>)/, count: 1 },
    ],
  },
];

function readVersion(): string {
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as { version?: unknown };
  const version = pkg.version;
  // Plain MAJOR.MINOR.PATCH only: iOS CFBundleVersion and Windows MSI reject
  // pre-release suffixes such as "-beta.1".
  if (typeof version !== "string" || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(`package.json version must be MAJOR.MINOR.PATCH, got ${JSON.stringify(version)}`);
  }
  return version;
}

function apply(source: string, rules: Rule[], version: string, path: string): string {
  let out = source;
  for (const { pattern, count } of rules) {
    const global = new RegExp(pattern.source, pattern.flags.includes("g") ? pattern.flags : pattern.flags + "g");
    const found = out.match(global)?.length ?? 0;
    if (found !== count) {
      throw new Error(`${path}: expected ${count} match(es) for ${pattern}, found ${found}`);
    }
    out = out.replace(global, (_m, before: string, after: string) => `${before}${version}${after}`);
  }
  return out;
}

function main(): void {
  const check = process.argv.includes("--check");
  const version = readVersion();
  const drifted: string[] = [];

  for (const { path, rules } of targets) {
    const file = join(root, path);
    const before = readFileSync(file, "utf8");
    const after = apply(before, rules, version, path);
    if (before === after) continue;
    drifted.push(path);
    if (!check) writeFileSync(file, after);
  }

  const where = relative(process.cwd(), join(root, "package.json")) || "package.json";
  if (drifted.length === 0) {
    console.log(`version ${version}: all files in sync with ${where}`);
  } else if (check) {
    console.error(`version ${version} (${where}) not applied to:\n  ${drifted.join("\n  ")}`);
    console.error("run: npm run version:sync");
    process.exit(1);
  } else {
    console.log(`version ${version}: updated\n  ${drifted.join("\n  ")}`);
  }
}

main();
