// Note that the specific exe here doesn't matter, we just need the same types found in all of them.
import type * as exe from "./exe/basicdd/basicdd.js";
import * as saves from "./saves.ts";
import { decrypt } from "./crypt.ts";

// Road Rash evaluation: the encrypted (GitHub Pages) copy's key, if any.
let KEY: Uint8Array | undefined;
let DIR_ = "";

// Loading card on the page (progress.ts): label and bytes so far.
const progress = (rrProgress: { label: string; loaded?: number; total?: number; done?: boolean }) =>
  self.postMessage({ rrProgress });

/** Fetch a whole file, reporting progress; `total` when the size is known in advance. */
async function fetchBytes(url: string, label: string, total?: number): Promise<Uint8Array> {
  const res = await fetch(url);
  total ??= Number(res.headers.get("Content-Length")) || undefined;
  if (!res.body) return new Uint8Array(await res.arrayBuffer());
  const reader = res.body.getReader();
  const chunks: Uint8Array[] = [];
  let loaded = 0, last = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    loaded += value.length;
    if (performance.now() - last > 100) {
      last = performance.now();
      progress({ label, loaded, total });
    }
  }
  progress({ label, loaded, total: total ?? loaded });
  const out = new Uint8Array(loaded);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}
const plain = (data: Uint8Array) => (KEY ? decrypt(KEY, data) : data);


// Road Rash evaluation: game files live in rr/ next to the page; workers are
// built into assets/, so resolve from one level up (static hosts like GitHub
// Pages can't rewrite paths the way the local server does).
let RR = new URL("../rr/", self.location.href).href;

// Road Rash evaluation: synchronous on-demand fetch of a lazily mounted file
// ("/RoadRash/<path>" -> "./rr/<path>"), called from Rust on first open.
(self as any).rr_fetch_sync = (path: string): Uint8Array | undefined => {
  const rel = path.replace(/^\/RoadRash\//i, "");
  const url = ((self as any).rr_lazy_names?.[rel.toLowerCase()]) ?? rel;
  // Tell the page, so it can show "Loading" if this takes a moment. Retry a
  // couple of times: a failed read here would end the game.
  self.postMessage({ rrFetch: { path: url, busy: true } });
  let xhr = new XMLHttpRequest();
  try {
    for (let attempt = 0; attempt < 3; attempt++) {
      xhr = new XMLHttpRequest();
      xhr.open("GET", `${RR}${url}`, false);
      xhr.responseType = "arraybuffer";
      try {
        xhr.send();
      } catch {
        continue;
      }
      if (xhr.status === 200) break;
    }
  } finally {
    self.postMessage({ rrFetch: { path: url, busy: false } });
  }
  return xhr.status === 200 ? plain(new Uint8Array(xhr.response)) : undefined;
};

(self as any).send_to_host = (func: string, args: any[], retAddr: number) => {
  const obj: exe.Msg = { func, args, retAddr };
  self.postMessage(obj);
};

async function run(name: string, memory: WebAssembly.Memory, query: string) {
  (self as any).rr_env = Object.fromEntries(new URLSearchParams(query));
  if ((self as any).rr_env.keys === "classic") (self as any).rr_env.RR_KEYS_CLASSIC = "1";
  const exe = await import(`./exe/${name}/${name}.js`);
  // Road Rash evaluation: threads. This worker is busy running the program, so
  // the page spawns thread workers on our behalf (same memory). On the encrypted
  // copy they get the compiled module, since the WebAssembly isn't there in the
  // clear; otherwise (module undefined) each thread loads it by URL.
  let module: WebAssembly.Module | undefined;
  if (KEY) {
    // The encrypted copy ships the WebAssembly encrypted, as rr/_wasm.bin.
    const wasm = plain(await fetchBytes(RR + "_wasm.bin", "Loading the game engine"));
    progress({ label: "Starting the engine" });
    module = await WebAssembly.compile(wasm);
    await exe.default({ module_or_path: module, memory });
  } else {
    progress({ label: "Loading the game engine" });
    await exe.default(/* module */ undefined, memory);
  }
  (self as any).rr_spawn_thread = (ptr: number) => {
    self.postMessage({ rrSpawn: { name, module, ptr, lazyNames: (self as any).rr_lazy_names, key: KEY, dir: DIR_ } });
  };
  if (name === "roadrash") {
    // Road Rash evaluation: mount the locally served game files before starting.
    const t0 = performance.now();
    progress({ label: "Reading the file list" });
    const list: string[] = await (await fetch(RR + "manifest.json")).json();
    let bytes = 0;
    // Large streamed files (the .RRA music) are fetched on first open instead.
    const lazy = (p: string) => /\.(rra|sf2|avi)$/i.test(p);
    const marker = new TextEncoder().encode("\0rr-lazy-file\0");
    const lazyNames: Record<string, string> = {};
    // Hosted copy: the start-up files come packed in one download
    // (bundle.json lists [path, size] in order); the local server has no
    // bundle, so each file is fetched on its own there.
    progress({ label: "Preparing the game files" });
    const index = await fetch(RR + "bundle.json");
    const packed: [string, number][] = index.ok ? await index.json() : [];
    if (packed.length) {
      const size = packed.reduce((n, [, s]) => n + s, 0);
      const all = plain(await fetchBytes(RR + "bundle.bin", "Loading the game files", size + (KEY ? 12 : 0)));
      let at = 0;
      for (const [path, size] of packed) {
        exe.mount_file(`/RoadRash/${path}`, all.subarray(at, at + size));
        at += size;
      }
      bytes += at;
    }
    const inBundle = new Set(packed.map(([path]) => path));
    for (const path of list) {
      if (lazy(path)) {
        exe.mount_file(`/RoadRash/${path}`, marker);
        lazyNames[path.toLowerCase()] = path;
        continue;
      }
      if (inBundle.has(path)) continue;
      const data = plain(new Uint8Array(await (await fetch(`${RR}${path}`)).arrayBuffer()));
      bytes += data.length;
      exe.mount_file(`/RoadRash/${path}`, data);
    }
    (self as any).rr_lazy_names = lazyNames;
    // Files the game wrote in earlier sessions (saves, registry) override the originals.
    const restored = await saves.loadAll();
    for (const [path, data] of restored) exe.mount_file(path, data);
    if (restored.length) console.log(`rr-save: restored ${restored.length} saved file(s): ${restored.map((r) => r[0]).join(", ")}`);
    console.log(`rr: mounted ${list.length} files, ${bytes} bytes in ${Math.round(performance.now() - t0)} ms`);
    progress({ label: "Starting", done: true });
    const trace = (self as any).rr_env.trace;
    if (trace) exe.set_trace(trace);
    // cwd stays "/" (= C:\ root); the game itself chdirs to the CD folder.
  }
  exe.main();
}

export interface StartMessage {
  name: string;
  memory: WebAssembly.Memory;
  query: string;
  key?: Uint8Array;
  dir?: string;
}

self.onmessage = (e: MessageEvent<StartMessage>) => {
  const { name, memory, query, key, dir } = e.data;
  KEY = key;
  DIR_ = dir ?? "";
  if (dir) RR = new URL(`../rr/${dir}`, self.location.href).href;
  run(name, memory, query).catch((e) => console.error(e));
};
