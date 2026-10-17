import { defineConfig } from "vite";
import fs from "node:fs";
import path from "node:path";

// Road Rash evaluation: local hosting of the translated game.
//  - cross-origin isolation (needed for SharedArrayBuffer) on every response,
//    including vite's internal module URLs (WebKit checks worker scripts);
//  - the game's data files served from the local copy at /rr/, with
//    /rr/manifest.json generated from the directory (never bundled/copied).
const GAME_DIR = process.env.RR_GAME_DIR ?? path.resolve(__dirname, "../../game/ROADRASH");

function walk(dir, base = "") {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    // Skips the RoadRash -> . symlink (native CD emulation) too.
    if (e.name.startsWith(".") || e.name === "Saves" || e.isSymbolicLink()) continue;
    const rel = base ? `${base}/${e.name}` : e.name;
    if (e.isDirectory()) {
      // RoadRash -> . symlink (native CD emulation) would recurse forever.
      if (fs.lstatSync(path.join(dir, e.name)).isSymbolicLink()) continue;
      out.push(...walk(path.join(dir, e.name), rel));
    } else if (!/\.(exe|log|ppm)$/i.test(e.name) && e.name !== "manifest.json") {
      out.push(rel);
    }
  }
  return out;
}

function middleware(req, res, next) {
  res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  res.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  res.setHeader("Cross-Origin-Resource-Policy", "same-origin");
  const url = decodeURIComponent((req.url ?? "").split("?")[0]);
  const m = url.match(/\/rr\/(.*)$/);
  if (!m) return next();
  if (m[1] === "manifest.json") {
    res.setHeader("Content-Type", "application/json");
    res.end(JSON.stringify(walk(GAME_DIR)));
    return;
  }
  const file = path.join(GAME_DIR, m[1]);
  if (!file.startsWith(GAME_DIR) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
    res.statusCode = 404;
    res.end();
    return;
  }
  // Real types for the assets the page itself uses (side panels).
  const ext = path.extname(file).toLowerCase();
  const type = ext === ".rri" || ext === ".jpg" ? "image/jpeg"
    : /\/text\/(fut[bdr]|badloc)\.dll$/i.test(file) ? "font/ttf"
    : "application/octet-stream";
  res.setHeader("Content-Type", type);
  res.setHeader("Content-Length", fs.statSync(file).size);
  fs.createReadStream(file).pipe(res);
}

const roadrash = {
  name: "roadrash-local-hosting",
  configureServer(server) {
    server.middlewares.use(middleware);
  },
  configurePreviewServer(server) {
    server.middlewares.use(middleware);
  },
};

export default defineConfig({
  base: "./",
  plugins: [roadrash],
  worker: { format: "es" },
  build: { target: "esnext" },
});
