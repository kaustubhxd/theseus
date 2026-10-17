// Build the GitHub Pages copy straight into git (fast-import), no plain copy on disk:
//   node build.mjs | git -C site fast-import --quiet
// Public: the loader page and file names. Encrypted (ChaCha20, key = PBKDF2 of
// the password in .password): the WebAssembly and every game file.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { execFileSync } from "node:child_process";

const HOME = process.env.HOME;
const DIST = `${HOME}/theseus-roadrash/theseus/web/dist`;
const GAME = `${HOME}/theseus-roadrash/game/ROADRASH`;
const HERE = `${HOME}/theseus-roadrash/deploy-gh`;

const password = fs.readFileSync(`${HERE}/.password`, "utf8").trim();
const salt = fs.readFileSync(`${HERE}/salt`, "utf8").trim();
const iterations = 600000;
const key = crypto.pbkdf2Sync(password, Buffer.from(salt, "base64"), iterations, 32, "sha256");

// Nonce from the key, path and content: deterministic (unchanged files give
// identical blobs between builds) and never reused for different content.
function encrypt(name, data) {
  const nonce = crypto.createHmac("sha256", key).update(name).update(crypto.createHash("sha256").update(data).digest()).digest().subarray(0, 12);
  const c = crypto.createCipheriv("chacha20", key, Buffer.concat([Buffer.alloc(4), nonce]));
  return Buffer.concat([nonce, c.update(data), c.final()]);
}

function walk(d, b = "") {
  let o = [];
  for (const e of fs.readdirSync(d, { withFileTypes: true })) {
    if (e.name.startsWith(".") || e.name === "Saves" || e.isSymbolicLink()) continue;
    const r = b ? `${b}/${e.name}` : e.name;
    if (e.isDirectory()) o.push(...walk(path.join(d, e.name), r));
    else if (!/\.(exe|log|ppm)$/i.test(e.name) && e.name !== "manifest.json") o.push(r);
  }
  return o;
}

const out = process.stdout;
let mark = 0;
const files = []; // [path, mark]
// Encrypted files live in rr/<dir>, one folder per salt (= per password), so a
// browser never mixes cached files from two keys. Encryption names stay
// "rr/<path>", so unchanged files encrypt identically.
const DIR = "v" + crypto.createHash("sha256").update(salt).digest("hex").slice(0, 8) + "/";
const inDir = (name) => name.replace(/^rr\//, `rr/${DIR}`);
// RR_PREFIX=test/ builds into a subfolder next to the live site (same blobs, so
// no extra repo size), for trying a change on the real host first.
const PREFIX = process.env.RR_PREFIX ?? "";
function blob(name, data) {
  name = PREFIX + name;
  mark++;
  out.write(`blob\nmark :${mark}\ndata ${data.length}\n`);
  out.write(data);
  out.write("\n");
  files.push([name, mark]);
}

// Loader page: everything vite built except the WebAssembly.
for (const f of walk(DIST)) {
  const data = fs.readFileSync(`${DIST}/${f}`);
  if (f.endsWith(".wasm")) blob(inDir("rr/_wasm.bin"), encrypt("rr/_wasm.bin", data));
  else blob(f, data);
}
blob(".nojekyll", Buffer.alloc(0));
blob("rr/crypt.json", Buffer.from(JSON.stringify({ salt, iterations, check: encrypt("check", Buffer.from("roadrash-ok")).toString("base64"), dir: DIR })));

const list = walk(GAME);
const lazy = (p) => /\.(rra|sf2|avi)$/i.test(p);
blob(inDir("rr/manifest.json"), Buffer.from(JSON.stringify(list)));
const eager = list.filter((p) => !lazy(p));
const parts = eager.map((p) => fs.readFileSync(`${GAME}/${p}`));
blob(inDir("rr/bundle.json"), Buffer.from(JSON.stringify(eager.map((p, i) => [p, parts[i].length]))));
blob(inDir("rr/bundle.bin"), encrypt("rr/bundle.bin", Buffer.concat(parts)));
// The page's own assets (fonts, panel art) and the files read on demand.
const pageAssets = (p) => /^TEXT\/(BADLOC|FUT[BDR])\.DLL$/i.test(p) || /^IMAGES\/CHARS\/[A-Z]+_H\.RRI$/i.test(p) || p === "IMAGES/SPLASH.RRI";
for (const p of list.filter((p) => lazy(p) || pageAssets(p))) blob(inDir(`rr/${p}`), encrypt(`rr/${p}`, fs.readFileSync(`${GAME}/${p}`)));

const msg = Buffer.from(`Road Rash (Theseus port), encrypted build\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\n`);
const now = Math.floor(Date.now() / 1000);
const gitConfig = (k) => execFileSync("git", ["-C", `${HERE}/site`, "config", k]).toString().trim();
out.write(`commit refs/heads/main\ncommitter ${gitConfig("user.name")} <${gitConfig("user.email")}> ${now} +0000\ndata ${msg.length}\n`);
out.write(msg);
// Build on what is on GitHub (fetched into refs/remotes/origin/main by
// publish.sh), so a push only uploads what changed and never needs --force.
// Files the build doesn't make (e.g. a README added on GitHub) are kept.
let parent;
try {
  parent = execFileSync("git", ["-C", `${HERE}/site`, "rev-parse", "--verify", "-q", "refs/remotes/origin/main"]).toString().trim();
} catch {}
if (parent) out.write(`from ${parent}\n`);
out.write("deleteall\n");
const generatedHere = (p) => p === ".nojekyll" || p === "index.html" || /^(assets|rr)\//.test(p) || /\.js$/.test(p);
// A main build also drops the test/ copy.
const generated = (p) => (PREFIX === "" && p.startsWith("test/")) || (p.startsWith(PREFIX) && generatedHere(p.slice(PREFIX.length)));
if (parent) {
  const tree = execFileSync("git", ["-C", `${HERE}/site`, "ls-tree", "-r", parent]).toString().trim().split("\n");
  for (const line of tree) {
    const [meta, name] = line.split("\t");
    const [mode, , sha] = meta.split(" ");
    if (!generated(name) && !files.some(([f]) => f === name)) out.write(`M ${mode} ${sha} ${name}\n`);
  }
}
for (const [name, m] of files) out.write(`M 100644 :${m} ${name}\n`);
out.write("\n");
process.stderr.write(`${files.length} files\n`);
