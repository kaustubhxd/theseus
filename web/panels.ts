// Road Rash evaluation: side panels in the margins around the game, styled
// with the game's own fonts (Badloc, Futura Condensed) and artwork.

const CLASSIC = new URLSearchParams(location.search).get("keys") === "classic";

type Row = [keys: string[], label: string];
const SECTIONS: [string, Row[]][] = [
  ["Ride", CLASSIC ? [
    [["↑"], "Accelerate"], [["↓"], "Brake"], [["←", "→"], "Steer"],
    [["N"], "Nitro"], [["D"], "Dismount"],
  ] : [
    [["W"], "Accelerate"], [["S"], "Brake"], [["A", "D"], "Steer"],
    [["⇧"], "Nitro"], [["F"], "Get off bike"],
  ]],
  ["Fight", CLASSIC ? [
    [["Ins"], "Punch / grab"], [["Space"], "Backhand"], [["Enter"], "Kick"],
  ] : [
    [["⌘"], "Punch / grab"], [["X"], "Backhand"], [["Space"], "Kick"],
  ]],
  ["Game", [
    [["Esc"], "Pause / quit"], [["Enter"], "Menu select"], [["F10"], "Dashboard"], [["+", "−"], "Detail (keypad)"],
    [CLASSIC ? ["↑", "↓"] : ["W", "S"], "Menu move"],
  ]],
];

// A few rider remarks, in the spirit of the game's bar gossip.
export const RIDERS: [string, string][] = [
  ["JON", "The fastest way through the City is on the one-way streets."],
  ["PEARL", "Oncoming traffic is an effective means of slowing down your opponents."],
  ["BOSE", "I don't confront the cops, I leave 'em behind."],
  ["RHONDA", "Better stay off the dirt roads. Not much traction out there."],
  ["MIKE", "Try squeezin' the brakes now and then."],
  ["AXLE", "When it comes to takin' out a competitor, timing is everything."],
  ["CYDNEY", "Catchin' some air can save you some time."],
  ["JIM", "Brake into turns and keep your eyes peeled for oncoming cars."],
];

import { pageAsset } from "./crypt.ts";

const FONTS: [family: string, file: string, weight: string][] = [
  ["Badloc", "BADLOC.DLL", "normal"],
  ["Futura RR", "FUTR.DLL", "normal"],
  ["Futura RR", "FUTB.DLL", "bold"],
  ["Futura RR Demi", "FUTD.DLL", "normal"],
];

// The game's TrueType fonts (TEXT/*.DLL), decrypted on the encrypted copy.
// Shared with the loading screen, so loaded once.
let fonts: Promise<void> | undefined;
export function loadFonts(): Promise<void> {
  fonts ??= (async () => {
    for (const [family, file, weight] of FONTS) {
      const face = new FontFace(family, await pageAsset(`TEXT/${file}`), { weight });
      document.fonts.add(await face.load());
    }
  })();
  return fonts;
}

// Panel art as a blob URL (works for plain and encrypted copies alike).
async function artUrl(path: string): Promise<string> {
  return URL.createObjectURL(new Blob([await pageAsset(path)], { type: "image/jpeg" }));
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

export function setupPanels(canvas: HTMLCanvasElement) {
  loadFonts().catch((e) => console.warn("rr: fonts", e));
  const left = el("aside", "rr-panel rr-left");
  const card = el("div", "rr-card");
  card.append(el("div", "rr-title", "Controls"));
  for (const [name, rows] of SECTIONS) {
    card.append(el("div", "rr-section", name));
    for (const [keys, label] of rows) {
      const row = el("div", "rr-row");
      const caps = el("span", "rr-keys");
      for (const k of keys) caps.append(el("kbd", k.length > 2 ? "rr-wide" : "", k));
      row.append(caps, el("span", "rr-label", label));
      card.append(row);
    }
  }
  card.append(el("div", "rr-foot", CLASSIC ? "classic keys · remove ?keys=classic for WASD" : "add ?keys=classic for the original keys"));
  left.append(card);

  const right = el("aside", "rr-panel rr-right");
  const frame = el("div", "rr-portrait");
  const imgs = [el("img"), el("img")];
  const quote = el("div", "rr-quote");
  const who = el("div", "rr-who");
  frame.append(...imgs, quote, who);
  const stack = el("div", "rr-stack");
  stack.append(el("div", "rr-title", "Word on the Street"), frame);
  right.append(stack);
  document.body.append(left, right);

  // Rotate riders: cross-fade between two <img>s.
  let i = 0, front = 0;
  const show = () => {
    const [name, line] = RIDERS[i % RIDERS.length];
    const next = imgs[1 - front];
    next.onload = () => {
      next.classList.add("rr-on");
      imgs[front].classList.remove("rr-on");
      front = 1 - front;
      quote.textContent = `“${line}”`;
      who.textContent = name;
    };
    artUrl(`IMAGES/CHARS/${name}_H.RRI`).then((url) => {
      if (next.src.startsWith("blob:")) URL.revokeObjectURL(next.src);
      next.src = url;
    });
    i++;
  };
  show();
  setInterval(show, 9000);

  // Size the panels to the margins; hide them when there's no room.
  const layout = () => {
    const r = canvas.getBoundingClientRect();
    const margin = r.left;
    for (const p of [left, right]) {
      p.style.width = `${Math.max(0, margin)}px`;
      p.style.display = margin >= 150 ? "flex" : "none";
    }
    left.style.setProperty("--scale", `${Math.min(1.25, Math.max(0.5, Math.min(margin / 300, r.height / 760)))}`);
    right.style.setProperty("--scale", left.style.getPropertyValue("--scale"));
  };
  new ResizeObserver(layout).observe(canvas);
  window.addEventListener("resize", layout);
  layout();
}
