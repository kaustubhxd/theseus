// Serve the built site from the git repo (no checkout) the way GitHub Pages
// would: under /roadrash/, static files only, no COOP/COEP headers, gzip for
// text types, max-age=600, and optional latency (DELAY ms) to mimic a network.
import http from "node:http";
import zlib from "node:zlib";
import { execFileSync } from "node:child_process";
const REF = process.env.REF ?? "main";
const DELAY = Number(process.env.DELAY ?? 0);
const TYPES = { html: "text/html", js: "text/javascript", json: "application/json", css: "text/css" };
http.createServer((req, res) => {
  let p = decodeURIComponent(new URL(req.url, "http://x").pathname);
  if (!p.startsWith("/roadrash/")) { res.writeHead(404); return res.end(); }
  p = p.slice("/roadrash/".length) || "index.html";
  if (process.env.LOG) console.log("GET", p);
  setTimeout(() => {
    try {
      let data = execFileSync("git", ["-C", "site", "cat-file", "blob", `${REF}:${p}`], { maxBuffer: 1 << 30 });
      const type = TYPES[p.split(".").pop()];
      const headers = { "Content-Type": type ?? "application/octet-stream", "Cache-Control": "max-age=600" };
      if (type && /gzip/.test(req.headers["accept-encoding"] ?? "")) {
        data = zlib.gzipSync(data);
        headers["Content-Encoding"] = "gzip";
      }
      headers["Content-Length"] = data.length;
      res.writeHead(200, headers);
      res.end(data);
    } catch { res.writeHead(404); res.end(); }
  }, DELAY);
}).listen(8890, "127.0.0.1");
