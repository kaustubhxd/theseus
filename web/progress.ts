// Road Rash evaluation: loading screen shown from the password screen until
// the game starts, built from the game's own art: the title picture
// (IMAGES/SPLASH.RRI), its fonts, a damage-meter style bar and rider tips.
// The program worker reports bytes as it downloads.

import { pageAsset } from "./crypt.ts";
import { loadFonts, RIDERS } from "./panels.ts";

export interface Progress {
  label: string;
  loaded?: number;
  total?: number;
  done?: boolean;
}

let screen: HTMLElement | undefined;
let tipTimer: number | undefined;

function build(): HTMLElement {
  const el = document.createElement("div");
  el.className = "rr-loading";
  el.innerHTML = `
    <div class="rr-lart"></div>
    <div class="rr-lband">
      <div class="rr-lrow"><span class="rr-llabel"></span><span class="rr-lbytes"></span></div>
      <div class="rr-meter"><div class="rr-meterfill"></div></div>
      <div class="rr-ltip"><span class="rr-ltext"></span><span class="rr-lwho"></span></div>
    </div>`;
  document.body.append(el);
  loadFonts().catch(() => {});
  pageAsset("IMAGES/SPLASH.RRI")
    .then((data) => {
      const art = el.querySelector<HTMLElement>(".rr-lart")!;
      art.style.backgroundImage = `url(${URL.createObjectURL(new Blob([data], { type: "image/jpeg" }))})`;
      art.classList.add("rr-on");
    })
    .catch(() => {});
  let i = Math.floor(Math.random() * RIDERS.length);
  const tip = () => {
    const [who, line] = RIDERS[i++ % RIDERS.length];
    el.querySelector(".rr-ltext")!.textContent = `“${line}”`;
    el.querySelector(".rr-lwho")!.textContent = who;
  };
  tip();
  tipTimer = window.setInterval(tip, 5000);
  return el;
}

export function showProgress(p: Progress) {
  if (p.done) {
    const el = screen;
    screen = undefined;
    clearInterval(tipTimer);
    el?.classList.add("rr-gone");
    setTimeout(() => el?.remove(), 600);
    return;
  }
  screen ??= build();
  screen.querySelector(".rr-llabel")!.textContent = p.label;
  const fill = screen.querySelector<HTMLElement>(".rr-meterfill")!;
  const bytes = screen.querySelector(".rr-lbytes")!;
  const mb = (n: number) => (n / 1048576).toFixed(1);
  if (p.total) {
    fill.classList.remove("rr-indet");
    fill.style.width = `${Math.min(100, (100 * (p.loaded ?? 0)) / p.total)}%`;
    bytes.textContent = `${mb(p.loaded ?? 0)} / ${mb(p.total)} MB`;
  } else {
    fill.classList.add("rr-indet");
    fill.style.width = "";
    bytes.textContent = p.loaded ? `${mb(p.loaded)} MB` : "";
  }
}

// ---- In-game loading indicator ----
// Between the menus and a race (and back) the game shows a black screen while
// it loads, and the hosted copy may be downloading the race's music or video.
// Show "Loading" over the game when the screen has been black for a moment,
// naming the download if one is under way.

let hint: HTMLElement | undefined;
let darkSince = 0;
let fetching: { what: string; since: number } | undefined;

function hintEl(): HTMLElement {
  if (!hint) {
    hint = document.createElement("div");
    hint.className = "rr-ghint";
    hint.innerHTML = `<div class="rr-ghtitle">Loading</div><div class="rr-ghwhat"></div>
      <div class="rr-meter"><div class="rr-meterfill rr-indet"></div></div>`;
    document.body.append(hint);
  }
  return hint;
}

function update() {
  const now = performance.now();
  const dark = darkSince && now - darkSince > 500;
  const slow = fetching && now - fetching.since > 250;
  // Only over a black screen: music may download mid-race while the game runs on.
  const show = !screen && dark;
  const el = hintEl();
  el.classList.toggle("rr-on", !!show);
  el.querySelector(".rr-ghwhat")!.textContent = slow ? fetching!.what : "";
}

/** A game file download started (busy) or finished, from rr_fetch_sync. */
export function fileFetch(path: string, busy: boolean) {
  const kind = /\.avi$/i.test(path) ? "video" : /\.rra$/i.test(path) ? "music" : /\.sf2$/i.test(path) ? "instruments" : "data";
  fetching = busy ? { what: `Fetching ${kind}…`, since: performance.now() } : undefined;
  update();
}

/** Watch the game canvas: a tiny copy, sampled 5 times a second, says whether it's black. */
export function watchCanvas(canvas: HTMLCanvasElement) {
  const probe = document.createElement("canvas");
  probe.width = 16;
  probe.height = 12;
  const pc = probe.getContext("2d", { willReadFrequently: true })!;
  setInterval(() => {
    pc.drawImage(canvas, 0, 0, 16, 12);
    const px = pc.getImageData(0, 0, 16, 12).data;
    let max = 0;
    for (let i = 0; i < px.length; i += 4) max = Math.max(max, px[i], px[i + 1], px[i + 2]);
    const isDark = max < 24;
    if (isDark && !darkSince) darkSince = performance.now();
    if (!isDark) darkSince = 0;
    update();
  }, 200);
}
