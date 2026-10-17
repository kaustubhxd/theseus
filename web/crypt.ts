// Road Rash evaluation: the GitHub Pages copy is public, so the WebAssembly and
// the game files there are encrypted (ChaCha20, key from the password with
// PBKDF2). Decryption has to be synchronous because game files are read with
// synchronous XHR from Rust, hence a plain-JS cipher rather than WebCrypto.
// Encrypted file layout: 12-byte nonce, then the ciphertext (block counter 0).
// The local vite server has no rr/crypt.json, so nothing here is used there.

export interface CryptConfig {
  salt: string; // base64
  iterations: number;
  check: string; // base64 encrypted "roadrash-ok"
  dir: string; // folder under rr/ holding this key's files, e.g. "v1a2b3c4d/"
}

// One ChaCha20 block (RFC 8439) into `out`, all in locals: this runs once per
// 64 bytes of game data, so it must not allocate.
function block(k: Uint32Array, counter: number, n: Uint32Array, out: Uint32Array) {
  const j0 = 0x61707865, j1 = 0x3320646e, j2 = 0x79622d32, j3 = 0x6b206574;
  const j4 = k[0], j5 = k[1], j6 = k[2], j7 = k[3], j8 = k[4], j9 = k[5], j10 = k[6], j11 = k[7];
  const j12 = counter | 0, j13 = n[0], j14 = n[1], j15 = n[2];
  let x0 = j0, x1 = j1, x2 = j2, x3 = j3, x4 = j4, x5 = j5, x6 = j6, x7 = j7;
  let x8 = j8, x9 = j9, x10 = j10, x11 = j11, x12 = j12, x13 = j13, x14 = j14, x15 = j15;
  let t: number;
  for (let i = 0; i < 10; i++) {
    x0 = (x0 + x4) | 0; t = x12 ^ x0; x12 = (t << 16) | (t >>> 16);
    x8 = (x8 + x12) | 0; t = x4 ^ x8; x4 = (t << 12) | (t >>> 20);
    x0 = (x0 + x4) | 0; t = x12 ^ x0; x12 = (t << 8) | (t >>> 24);
    x8 = (x8 + x12) | 0; t = x4 ^ x8; x4 = (t << 7) | (t >>> 25);
    x1 = (x1 + x5) | 0; t = x13 ^ x1; x13 = (t << 16) | (t >>> 16);
    x9 = (x9 + x13) | 0; t = x5 ^ x9; x5 = (t << 12) | (t >>> 20);
    x1 = (x1 + x5) | 0; t = x13 ^ x1; x13 = (t << 8) | (t >>> 24);
    x9 = (x9 + x13) | 0; t = x5 ^ x9; x5 = (t << 7) | (t >>> 25);
    x2 = (x2 + x6) | 0; t = x14 ^ x2; x14 = (t << 16) | (t >>> 16);
    x10 = (x10 + x14) | 0; t = x6 ^ x10; x6 = (t << 12) | (t >>> 20);
    x2 = (x2 + x6) | 0; t = x14 ^ x2; x14 = (t << 8) | (t >>> 24);
    x10 = (x10 + x14) | 0; t = x6 ^ x10; x6 = (t << 7) | (t >>> 25);
    x3 = (x3 + x7) | 0; t = x15 ^ x3; x15 = (t << 16) | (t >>> 16);
    x11 = (x11 + x15) | 0; t = x7 ^ x11; x7 = (t << 12) | (t >>> 20);
    x3 = (x3 + x7) | 0; t = x15 ^ x3; x15 = (t << 8) | (t >>> 24);
    x11 = (x11 + x15) | 0; t = x7 ^ x11; x7 = (t << 7) | (t >>> 25);
    x0 = (x0 + x5) | 0; t = x15 ^ x0; x15 = (t << 16) | (t >>> 16);
    x10 = (x10 + x15) | 0; t = x5 ^ x10; x5 = (t << 12) | (t >>> 20);
    x0 = (x0 + x5) | 0; t = x15 ^ x0; x15 = (t << 8) | (t >>> 24);
    x10 = (x10 + x15) | 0; t = x5 ^ x10; x5 = (t << 7) | (t >>> 25);
    x1 = (x1 + x6) | 0; t = x12 ^ x1; x12 = (t << 16) | (t >>> 16);
    x11 = (x11 + x12) | 0; t = x6 ^ x11; x6 = (t << 12) | (t >>> 20);
    x1 = (x1 + x6) | 0; t = x12 ^ x1; x12 = (t << 8) | (t >>> 24);
    x11 = (x11 + x12) | 0; t = x6 ^ x11; x6 = (t << 7) | (t >>> 25);
    x2 = (x2 + x7) | 0; t = x13 ^ x2; x13 = (t << 16) | (t >>> 16);
    x8 = (x8 + x13) | 0; t = x7 ^ x8; x7 = (t << 12) | (t >>> 20);
    x2 = (x2 + x7) | 0; t = x13 ^ x2; x13 = (t << 8) | (t >>> 24);
    x8 = (x8 + x13) | 0; t = x7 ^ x8; x7 = (t << 7) | (t >>> 25);
    x3 = (x3 + x4) | 0; t = x14 ^ x3; x14 = (t << 16) | (t >>> 16);
    x9 = (x9 + x14) | 0; t = x4 ^ x9; x4 = (t << 12) | (t >>> 20);
    x3 = (x3 + x4) | 0; t = x14 ^ x3; x14 = (t << 8) | (t >>> 24);
    x9 = (x9 + x14) | 0; t = x4 ^ x9; x4 = (t << 7) | (t >>> 25);
  }
  out[0] = x0 + j0; out[1] = x1 + j1; out[2] = x2 + j2; out[3] = x3 + j3;
  out[4] = x4 + j4; out[5] = x5 + j5; out[6] = x6 + j6; out[7] = x7 + j7;
  out[8] = x8 + j8; out[9] = x9 + j9; out[10] = x10 + j10; out[11] = x11 + j11;
  out[12] = x12 + j12; out[13] = x13 + j13; out[14] = x14 + j14; out[15] = x15 + j15;
}

/** Decrypt nonce||ciphertext with a 32-byte key. */
export function decrypt(key: Uint8Array, data: Uint8Array): Uint8Array {
  const k = new Uint32Array(8);
  const kv = new DataView(key.buffer, key.byteOffset, 32);
  for (let i = 0; i < 8; i++) k[i] = kv.getUint32(i * 4, true);
  const nv = new DataView(data.buffer, data.byteOffset, 12);
  const n = new Uint32Array([nv.getUint32(0, true), nv.getUint32(4, true), nv.getUint32(8, true)]);
  // Copy into a fresh, 4-byte aligned buffer and XOR whole words (little-endian
  // host, like every browser this runs on); the tail goes byte by byte.
  const len = data.length - 12;
  const out = new Uint8Array((len + 3) & ~3);
  out.set(data.subarray(12));
  const words = new Uint32Array(out.buffer);
  const ks = new Uint32Array(16);
  for (let w = 0, counter = 0; w < words.length; w += 16, counter++) {
    block(k, counter, n, ks);
    const end = Math.min(16, words.length - w);
    for (let i = 0; i < end; i++) words[w + i] ^= ks[i];
  }
  return out.subarray(0, len);
}

function b64(s: string): Uint8Array {
  return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
}

export async function deriveKey(password: string, cfg: CryptConfig): Promise<Uint8Array> {
  const base = await crypto.subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, ["deriveBits"]);
  const bits = await crypto.subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: b64(cfg.salt), iterations: cfg.iterations }, base, 256);
  return new Uint8Array(bits);
}

export function keyMatches(key: Uint8Array, cfg: CryptConfig): boolean {
  return new TextDecoder().decode(decrypt(key, b64(cfg.check))) === "roadrash-ok";
}

/** The page's key once unlocked (undefined on the local server). */
export let KEY: Uint8Array | undefined;
/** Folder under rr/ with the encrypted files ("" on the local server). Each
 *  password gets its own, so a browser can't mix cached files from two keys. */
export let DIR = "";

/** A game file for the page itself (fonts, panel art): bytes, decrypted if needed. */
export async function pageAsset(path: string): Promise<Uint8Array> {
  const data = new Uint8Array(await (await fetch(`./rr/${DIR}${path}`)).arrayBuffer());
  return KEY ? decrypt(KEY, data) : data;
}

const SAVED = "rr-key";

/** On the encrypted copy, ask for the password (once per browser) and return the key. */
export async function unlock(): Promise<Uint8Array | undefined> {
  const res = await fetch("./rr/crypt.json", { cache: "no-store" }).catch(() => undefined);
  if (!res?.ok) return undefined;
  const cfg: CryptConfig = await res.json();
  DIR = cfg.dir;
  // A private link carries the key after "#k=" (never sent to the server):
  // opening it once unlocks this browser for good, no password to type.
  const fromLink = new URLSearchParams(location.hash.slice(1)).get("k");
  if (fromLink) history.replaceState(null, "", location.pathname + location.search);
  const saved = fromLink ?? localStorage.getItem(SAVED);
  if (saved && /^[0-9a-f]{64}$/i.test(saved)) {
    const key = Uint8Array.from(saved.match(/../g)!.map((h) => parseInt(h, 16)));
    if (keyMatches(key, cfg)) {
      localStorage.setItem(SAVED, saved.toLowerCase());
      return (KEY = key);
    }
  }
  const form = document.createElement("form");
  form.className = "rr-unlock";
  form.innerHTML = `<h1>Road Rash</h1><p class="rr-err"></p>
    <input type="password" placeholder="Password" autocomplete="current-password" autofocus>
    <button>Ride</button>`;
  document.body.append(form);
  const input = form.querySelector("input")!;
  const err = form.querySelector(".rr-err")!;
  input.focus();
  const key = await new Promise<Uint8Array>((resolve) => {
    form.onsubmit = async (e) => {
      e.preventDefault();
      err.textContent = "Unlocking…";
      const k = await deriveKey(input.value, cfg);
      if (keyMatches(k, cfg)) resolve(k);
      else err.textContent = "Wrong password.";
    };
  });
  localStorage.setItem(SAVED, [...key].map((b) => b.toString(16).padStart(2, "0")).join(""));
  form.remove();
  return (KEY = key);
}
