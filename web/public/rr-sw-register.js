// Road Rash evaluation: install the page's service worker (rr-sw.js). Until it
// controls the page, cross-origin isolation is off, so reload once when it
// takes over (as coi-serviceworker did).
if ("serviceWorker" in navigator) {
  if (!navigator.serviceWorker.controller) {
    navigator.serviceWorker.addEventListener("controllerchange", () => {
      if (!self.crossOriginIsolated) location.reload();
    }, { once: true });
  }
  navigator.serviceWorker.register("rr-sw.js").catch((e) => console.warn("rr: service worker", e));
}
