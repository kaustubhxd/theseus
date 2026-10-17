// Road Rash evaluation: a Web Worker running one program thread.
// The page creates it with the program's module and shared memory, then the
// thread calls into the host through the same synchronous channel as the
// main program worker.
import type * as exe from "./exe/basicdd/basicdd.js";
import { decrypt } from "./crypt.ts";

// Road Rash evaluation: the encrypted (GitHub Pages) copy's key, if any.
let KEY: Uint8Array | undefined;


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
  if (xhr.status !== 200) return undefined;
  const data = new Uint8Array(xhr.response);
  return KEY ? decrypt(KEY, data) : data;
};

(self as any).send_to_host = (func: string, args: any[], retAddr: number) => {
  const obj: exe.Msg = { func, args, retAddr };
  self.postMessage(obj);
};

self.onmessage = async (e: MessageEvent) => {
  const { name, module, memory, ptr, lazyNames, key, dir } = e.data;
  KEY = key;
  if (dir) RR = new URL(`../rr/${dir}`, self.location.href).href;
  (self as any).rr_lazy_names = lazyNames;
  // Threads may start threads too; the page spawns them.
  (self as any).rr_spawn_thread = (p: number) => {
    self.postMessage({ rrSpawn: { name, module, ptr: p, lazyNames, key, dir } });
  };
  const prog = await import(`./exe/${name}/${name}.js`);
  await prog.default({ module_or_path: module, memory });
  prog.rr_thread_entry(ptr);
};
