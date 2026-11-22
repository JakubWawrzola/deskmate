// Holds the translations of the window to its source: every text that the
// code passes to t() or tn() must be in each src/i18n/<language>.json, no
// translation may keep a text the code no longer has, and a {placeholder}
// must survive translation. Run by `npm run lint`.
//
// The English text is the key, so there is no English file: the source is
// the code itself.

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const sourceDir = join(root, "src");
const i18nDir = join(sourceDir, "i18n");

/** Every .ts and .tsx file under `dir`. */
function sourceFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

/** The value of a JavaScript string literal, quotes included in `literal`. */
function unquote(literal) {
  const body = literal.slice(1, -1);
  return body.replace(/\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|.)/gs, (_, escape) => {
    if (escape === "n") return "\n";
    if (escape === "t") return "\t";
    if (escape.startsWith("u{")) return String.fromCodePoint(parseInt(escape.slice(2, -1), 16));
    if (escape.startsWith("u")) return String.fromCharCode(parseInt(escape.slice(1), 16));
    return escape;
  });
}

const errors = [];
const texts = new Set();
// t( or tn( followed by one string literal: "…", '…' or `…`.
const CALL =
  /\btn?\(\s*("(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`)/g;
// A call whose first argument is anything else cannot be checked.
const ANY_CALL = /\btn?\(\s*([^\s"'`)])/g;

for (const file of sourceFiles(sourceDir)) {
  if (file.startsWith(i18nDir)) continue;
  const code = readFileSync(file, "utf8");
  for (const match of code.matchAll(CALL)) {
    const literal = match[1];
    if (literal.startsWith("`") && literal.includes("${")) {
      errors.push(`${file}: t() takes a plain text, not a template with \${...}: ${literal}`);
      continue;
    }
    texts.add(unquote(literal));
  }
  for (const match of code.matchAll(ANY_CALL)) {
    const line = code.slice(0, match.index).split("\n").length;
    const rest = code.slice(match.index, match.index + 60).split("\n")[0];
    // t(CONSTANT[key]) and t(variable) show a text that tn() marked.
    if (!/^tn?\(\s*[A-Za-z_][\w.]*(\[[^\]]+\])?(\s*\?\?\s*[\w.]+)?\s*[,)]/.test(rest)) {
      errors.push(`${file}:${line}: cannot tell which text this shows: ${rest}`);
    }
  }
}

const placeholders = (text) => (text.match(/\{\w+\}/g) ?? []).sort().join(" ");

for (const name of readdirSync(i18nDir).filter((file) => file.endsWith(".json"))) {
  const translation = JSON.parse(readFileSync(join(i18nDir, name), "utf8"));
  // An empty file is a language that is not translated yet: the window does
  // not offer it. A translation that was started has to be complete.
  if (Object.keys(translation).length === 0) continue;
  for (const text of texts) {
    if (!(text in translation)) {
      errors.push(`${name}: no translation of ${JSON.stringify(text)}`);
    } else if (typeof translation[text] !== "string" || translation[text].trim() === "") {
      errors.push(`${name}: empty translation of ${JSON.stringify(text)}`);
    } else if (placeholders(translation[text]) !== placeholders(text)) {
      errors.push(`${name}: the placeholders differ in ${JSON.stringify(text)}`);
    }
  }
  for (const text of Object.keys(translation)) {
    if (!texts.has(text)) {
      errors.push(`${name}: translates a text the window no longer has: ${JSON.stringify(text)}`);
    }
  }
}

if (process.argv.includes("--list")) {
  // The texts, sorted, as a starting point for a new translation.
  console.log(JSON.stringify([...texts].sort(), null, 2));
} else if (errors.length > 0) {
  console.error(errors.slice(0, 80).join("\n"));
  if (errors.length > 80) console.error(`... and ${errors.length - 80} more`);
  console.error(`\ni18n check failed: ${errors.length} problem(s), ${texts.size} texts.`);
  process.exit(1);
} else {
  console.log(`i18n check passed: ${texts.size} texts in every language.`);
}
