// Road Rash evaluation: background download of the files the game otherwise
// fetches on demand (music, videos, instrument banks), so race transitions
// don't stall on the network. Files land in the service worker's cache
// (rr-sw.js), which then serves the game's own synchronous reads.

const CACHE = "rr-files-v1";

// Most useful first: instruments, race-start and crash videos, the music, then the rest.
const ORDER = [/\.sf2$/i, /VIDEO\/START\d/i, /VIDEO\/WRECK\d/i, /\.rra$/i, /VIDEO\/BUSTED\d/i, /VIDEO\/(WIN|LOSE)\d/i, /./];
const rank = (p: string) => ORDER.findIndex((re) => re.test(p));

let started = false;

export async function startPrefetch(dir: string) {
  if (started || !("caches" in self)) return;
  started = true;
  navigator.serviceWorker?.controller?.postMessage({ rrKeepDir: dir });
  // Ask the browser not to evict the cache under storage pressure.
  navigator.storage?.persist?.().catch(() => {});
  const base = new URL(`./rr/${dir}`, location.href).href;
  const list: string[] = await (await fetch(`${base}manifest.json`)).json();
  const wanted = list.filter((p) => /\.(rra|sf2|avi)$/i.test(p)).sort((a, b) => rank(a) - rank(b));
  const cache = await caches.open(CACHE);
  let next = 0, fetched = 0;
  const t0 = performance.now();
  const worker = async () => {
    while (next < wanted.length) {
      const url = base + wanted[next++];
      if (await cache.match(url)) continue;
      try {
        // Goes through the service worker, which stores it; just drain the body.
        const res = await fetch(url, { priority: "low" } as RequestInit);
        await res.arrayBuffer();
        fetched++;
      } catch {
        // Offline or closed tab: try again next visit.
      }
    }
  };
  await Promise.all([worker(), worker()]);
  console.log(`rr-prefetch: ${fetched} new of ${wanted.length} files in ${Math.round((performance.now() - t0) / 1000)} s`);
}
