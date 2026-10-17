// Note that the specific exe here doesn't matter, we just need the same types found in all of them.
import type * as exe from "./exe/basicdd/basicdd.js";
import type * as worker from "./worker.js";
import * as saves from "./saves.ts";
import { setupPanels } from "./panels.ts";
import { unlock, DIR } from "./crypt.ts";
import { showProgress, watchCanvas, fileFetch } from "./progress.ts";
import { startPrefetch } from "./prefetch.ts";

// Road Rash evaluation: KeyboardEvent.code -> [PC set-1 scan code, VK_*, extended].
const KEYS: Record<string, [number, number, number]> = {
  MetaLeft: [0x5b, 0x5b, 1], MetaRight: [0x5c, 0x5c, 1],
  Escape: [0x01, 0x1b, 0], Enter: [0x1c, 0x0d, 0], Space: [0x39, 0x20, 0], Tab: [0x0f, 0x09, 0],
  Backspace: [0x0e, 0x08, 0],
  ArrowUp: [0x48, 0x26, 1], ArrowDown: [0x50, 0x28, 1], ArrowLeft: [0x4b, 0x25, 1], ArrowRight: [0x4d, 0x27, 1],
  ShiftLeft: [0x2a, 0xa0, 0], ShiftRight: [0x36, 0xa1, 0], ControlLeft: [0x1d, 0xa2, 0], ControlRight: [0x1d, 0xa3, 1],
  AltLeft: [0x38, 0xa4, 0], AltRight: [0x38, 0xa5, 1],
  Insert: [0x52, 0x2d, 1], Delete: [0x53, 0x2e, 1], Home: [0x47, 0x24, 1], End: [0x4f, 0x23, 1],
  NumpadAdd: [0x4e, 0x6b, 0], NumpadSubtract: [0x4a, 0x6d, 0], NumpadEnter: [0x1c, 0x0d, 1],
  ...Object.fromEntries("QWERTYUIOP".split("").map((c, i) => [`Key${c}`, [0x10 + i, c.charCodeAt(0), 0]])),
  ...Object.fromEntries("ASDFGHJKL".split("").map((c, i) => [`Key${c}`, [0x1e + i, c.charCodeAt(0), 0]])),
  ...Object.fromEntries("ZXCVBNM".split("").map((c, i) => [`Key${c}`, [0x2c + i, c.charCodeAt(0), 0]])),
  ...Object.fromEntries("1234567890".split("").map((c, i) => [`Digit${c}`, [0x02 + i, c.charCodeAt(0), 0]])),
  ...Object.fromEntries([...Array(10).keys()].map((i) => [`F${i + 1}`, [0x3b + i, 0x70 + i, 0]])),
};

// Road Rash evaluation: true while the save/load picker is shown.
let pickerOpen = false;

// Road Rash evaluation: the WASD layout (W/A/S/D = arrows, Cmd = Ins punch,
// Space = Enter kick, X = Space backhand, Left Shift = N nitro, F = D dismount)
// is applied on the game side, user32 rr_windows::map_key with host::rr_wasd;
// ?keys=classic turns it off there.

class MessageQueue {
  private messages: Event[] = [];
  private waiter: ((value: Event) => void) | undefined;

  poll(): Event | undefined {
    return this.messages.shift();
  }

  wait(): Promise<Event> {
    const msg = this.poll();
    if (msg !== undefined) {
      return Promise.resolve(msg);
    }
    const { promise, resolve } = Promise.withResolvers<Event>();
    this.waiter = resolve;
    return promise;
  }

  private enqueue = (e: Event) => {
    e.preventDefault();
    if (this.waiter) {
      this.waiter(e);
      this.waiter = undefined;
    } else {
      this.messages.push(e);
    }
  };
  private discard = (e: Event) => {
    e.preventDefault();
  };

  listen(dom: HTMLCanvasElement) {
    dom.onmousedown = this.enqueue;
    dom.onmouseup = this.enqueue;
    dom.onmousemove = this.enqueue;
    dom.oncontextmenu = this.discard;
    // Road Rash evaluation: keyboard input (the wasm side already parses it).
    const known = (e: KeyboardEvent) => KEYS[e.code];
    // macOS drops the keyup of keys pressed while Cmd is held, so release
    // those ourselves when Cmd (punch) comes up, and everything on blur.
    const held = new Map<string, boolean>(); // code -> pressed while Cmd was down
    const release = (only: (cmd: boolean) => boolean) => {
      for (const [code, cmd] of [...held]) {
        if (!only(cmd)) continue;
        held.delete(code);
        this.enqueue(new KeyboardEvent("keyup", { code }));
      }
    };
    window.addEventListener("keydown", (e) => {
      if (!known(e) || pickerOpen) return;
      if (!e.code.startsWith("Meta")) held.set(e.code, held.get(e.code) || e.metaKey);
      this.enqueue(e);
    });
    window.addEventListener("keyup", (e) => {
      if (!known(e) || pickerOpen) return;
      held.delete(e.code);
      this.enqueue(e);
      if (e.code.startsWith("Meta")) release((cmd) => cmd);
    });
    window.addEventListener("blur", () => release(() => true));
    // Cmd+W can't be blocked by a page; ask before leaving mid-game instead.
    window.addEventListener("beforeunload", (e) => e.preventDefault());
  }
}

class Host implements exe.WasmHost {
  consoleDom = document.createElement("pre");
  consoleOutputDecoder = new TextDecoder();
  consoleOutput = "";
  window_: HTMLCanvasElement | undefined;

  surfaces: Map<number, HTMLCanvasElement> = new Map();
  nextSurface = 1;
  messageQueue = new MessageQueue();

  constructor(public wasmMemory: WebAssembly.Memory) {
    this.consoleDom.id = "console";
    document.body.appendChild(this.consoleDom);
  }

  onMessage(e: MessageEvent<exe.Msg>) {
    const msg = e.data;
    const ret = (this as any)[msg.func](...msg.args);
    if (msg.retAddr) {
      if (ret instanceof Promise) {
        ret.then((ret) => this.finishSync(msg.retAddr, ret));
        return;
      }
      this.finishSync(msg.retAddr, ret);
    }
  }

  finishSync(retAddr: number, ret: number | number[]): void {
    const arr = Array.isArray(ret) ? ret : [ret];
    if (!Number.isFinite(arr[0]) || arr[0] == 0) {
      // For synchronization to work, we must put a non-zero value in the first slot.
      // If this hits we messed up the sync/non-sync ness of some API.
      throw new Error();
    }
    const ints = new Int32Array(this.wasmMemory.buffer, retAddr, arr.length);
    ints.set(arr);
    Atomics.notify(ints, 0, 1);
  }

  console_write(ptr: number, len: number): void {
    // We must copy from wasm memory to decode.
    const inBuf = new Uint8Array(this.wasmMemory.buffer, ptr, len).slice();
    this.consoleOutput += this.consoleOutputDecoder.decode(inBuf, { stream: true });
    this.consoleDom.innerText = this.consoleOutput;
  }

  create_surface(width: number, height: number): number {
    const surface = document.createElement("canvas");
    surface.width = width;
    surface.height = height;
    // Road Rash evaluation: off-screen surfaces stay off the page (they were
    // shown as debug canvases below the game).
    if (new URLSearchParams(location.search).has("debugSurfaces")) document.body.appendChild(surface);

    const id = this.nextSurface++;
    this.surfaces.set(id, surface);
    return id;
  }

  // Road Rash evaluation: play the program's audio ring buffer (stereo i16 in
  // shared wasm memory: u32 read, u32 write, u32 capacity, then samples).
  audioCtx: AudioContext | undefined;
  create_audio(ring: number, rate: number, channels: number): number {
    const ctx = new AudioContext({ sampleRate: rate });
    this.audioCtx = ctx;
    const node = ctx.createScriptProcessor(2048, 0, 2);
    let consumed = 0, sumSq = 0, lastLog = performance.now();
    node.onaudioprocess = (ev) => {
      const buf = this.wasmMemory.buffer;
      const idx = new Uint32Array(buf, ring, 3);
      const cap = idx[2];
      const data = new Int16Array(buf, ring + 12, cap);
      let r = Atomics.load(idx, 0);
      const w = Atomics.load(idx, 1);
      const left = ev.outputBuffer.getChannelData(0), right = ev.outputBuffer.getChannelData(1);
      for (let i = 0; i < left.length; i++) {
        if (((w - r) >>> 0) >= channels) {
          const l = data[r % cap] / 32768, rr = channels > 1 ? data[(r + 1) % cap] / 32768 : l;
          left[i] = l; right[i] = rr; sumSq += l * l;
          r = (r + channels) >>> 0; consumed++;
        } else {
          left[i] = 0; right[i] = 0;
        }
      }
      Atomics.store(idx, 0, r);
      const now = performance.now();
      if (now - lastLog > 2000) {
        console.log(`rr-audio: ${consumed} frames played, rms ${Math.round(Math.sqrt(sumSq / Math.max(1, consumed)) * 32768)}, state ${ctx.state}`);
        consumed = 0; sumSq = 0; lastLog = now;
      }
    };
    // Road Rash evaluation: sound on unless muted with the corner button
    // (remembered) or ?sound=0. Earlier, during testing, it was off by default; the pipeline still
    // runs (and logs), but nothing reaches the speakers by default.
    const gain = ctx.createGain();
    const muted = () => localStorage.getItem("rr-muted") === "1" || new URLSearchParams(location.search).get("sound") === "0";
    gain.gain.value = muted() ? 0 : 1;
    // Mute toggle in the bottom-right corner.
    const button = document.createElement("button");
    button.className = "rr-mute";
    const label = () => {
      button.textContent = gain.gain.value ? "🔊" : "🔇";
      button.title = gain.gain.value ? "Mute" : "Unmute";
    };
    label();
    button.onclick = () => {
      const now = gain.gain.value ? 0 : 1;
      gain.gain.value = now;
      localStorage.setItem("rr-muted", now ? "0" : "1");
      label();
      button.blur(); // keep the keyboard on the game
    };
    document.body.append(button);
    node.connect(gain);
    gain.connect(ctx.destination);
    // Browsers only start audio after a user gesture.
    const resume = () => ctx.resume();
    window.addEventListener("mousedown", resume);
    window.addEventListener("keydown", resume);
    ctx.resume();
    console.log(`rr-audio: created ${rate} Hz x${channels}, state ${ctx.state}, ${gain.gain.value ? "audible" : "muted"}`);
    return 1;
  }

  create_window(_title: string, width: number, height: number): number {
    this.window_ = document.createElement("canvas");
    this.window_.className = "window";
    this.window_.width = width;
    this.window_.height = height;
    document.body.appendChild(this.window_);
    this.messageQueue.listen(this.window_);
    setupPanels(this.window_);
    watchCanvas(this.window_);
    return 1;
  }

  resize_window(_id: number, width: number, height: number): void {
    this.window_!.width = width;
    this.window_!.height = height;
  }

  render(_window_id: number, surface_id: number) {
    const surface = this.surfaces.get(surface_id)!;
    this.window_!.getContext("2d")!.drawImage(surface, 0, 0);
  }

  set_pixels(id: number, ptr: number, len: number): number {
    // TODO: investigate using OffscreenCanvas here instead,
    // https://news.ycombinator.com/item?id=48297805
    const copy = new Uint8ClampedArray(
      this.wasmMemory.buffer,
      ptr,
      len,
    ).slice();
    // Road Rash evaluation: surface pixels carry no alpha (0); make them opaque,
    // as the native SDL path does by ignoring alpha.
    for (let i = 3; i < copy.length; i += 4) copy[i] = 255;
    const surface = this.surfaces.get(id)!;
    const imageData = new ImageData(copy, surface.width);
    surface.getContext("2d")!.putImageData(imageData, 0, 0);
    return 1;
  }

  private serializeMessage(event: Event): number[] {
    // see wasm.rs:parse_message
    const typeToCode: Record<string, number> = {
      mousedown: 2,
      mouseup: 3,
      mousemove: 4,
      keydown: 5,
      keyup: 6,
    };
    const code = typeToCode[event.type];
    if (code === undefined) throw new Error();
    switch (event.type) {
      case "mousedown":
      case "mouseup":
      case "mousemove": {
        const e = event as MouseEvent;
        // Road Rash evaluation: the canvas is scaled by CSS; map back to game pixels.
        const c = e.target as HTMLCanvasElement;
        const x = Math.floor((e.offsetX * c.width) / (c.clientWidth || c.width));
        const y = Math.floor((e.offsetY * c.height) / (c.clientHeight || c.height));
        return [typeToCode[e.type]!, x, y, 1 << e.button];
      }
      case "keydown":
      case "keyup": {
        const e = event as KeyboardEvent;
        // The physical key; the game side applies the WASD layout (it knows
        // when the game is taking typed text).
        const [sc, vk, ext] = KEYS[e.code]!;
        return [code, sc, vk, ext | (e.repeat ? 2 : 0), vk];
      }
      default:
        throw new Error();
    }
  }

  poll_message(): number[] {
    const event = this.messageQueue.poll();
    return event ? this.serializeMessage(event) : [-1];
  }

  async wait_message(): Promise<number[]> {
    const event = await this.messageQueue.wait();
    return this.serializeMessage(event);
  }

  // Road Rash evaluation: save/load picker shown over the game. Resolves to
  // [1, ...chars of the chosen file name] or [-1] when cancelled.
  choose_file(save: boolean, names: string[], proposed: string): Promise<number[]> {
    return new Promise((resolve) => {
      pickerOpen = true;
      const box = document.createElement("div");
      box.style.cssText = "position:fixed;inset:0;background:rgba(0,0,0,.75);display:flex;align-items:center;justify-content:center;z-index:10;font:16px sans-serif;color:#fff";
      const panel = document.createElement("div");
      panel.style.cssText = "background:#222;border:2px solid #f60;padding:16px;min-width:340px;max-height:420px;overflow:auto";
      panel.innerHTML = `<div style="font-size:20px;color:#f60;margin-bottom:10px">${save ? "Save game" : "Load game"}</div>`;
      const done = (name: string | null) => {
        pickerOpen = false;
        box.remove();
        resolve(name ? [1, ...[...name.slice(0, 63)].map((c) => c.charCodeAt(0)), 0] : [-1]);
      };
      let input: HTMLInputElement | undefined;
      if (save) {
        input = document.createElement("input");
        input.value = proposed.replace(/\.rrs$/i, "");
        input.style.cssText = "width:100%;font:16px sans-serif;margin-bottom:8px";
        input.onkeydown = (e) => { e.stopPropagation(); if (e.key === "Enter") done(input!.value.trim() || null); };
        panel.appendChild(input);
      }
      if (names.length) {
        const label = document.createElement("div");
        label.textContent = save ? "or overwrite:" : "Saved games:";
        label.style.cssText = "margin:6px 0;color:#aaa";
        panel.appendChild(label);
      } else if (!save) {
        panel.appendChild(Object.assign(document.createElement("div"), { textContent: "No saved games yet." }));
      }
      for (const n of names) {
        const b = document.createElement("button");
        b.textContent = n;
        b.style.cssText = "display:block;width:100%;margin:3px 0;font:15px sans-serif;text-align:left";
        b.onclick = () => done(n);
        panel.appendChild(b);
      }
      const row = document.createElement("div");
      row.style.cssText = "margin-top:10px;text-align:right";
      if (save) {
        const ok = Object.assign(document.createElement("button"), { textContent: "Save" });
        ok.onclick = () => done(input!.value.trim() || null);
        row.appendChild(ok);
      }
      const cancel = Object.assign(document.createElement("button"), { textContent: "Cancel" });
      cancel.style.marginLeft = "8px";
      cancel.onclick = () => done(null);
      row.appendChild(cancel);
      panel.appendChild(row);
      box.appendChild(panel);
      document.body.appendChild(box);
      input?.focus();
    });
  }

  // Road Rash evaluation: persist written files (saves, registry) in IndexedDB.
  write_file(path: string, ptr: number, len: number): number {
    const data = new Uint8Array(this.wasmMemory.buffer, ptr, len).slice();
    saves.saveFile(path, data).then(
      () => console.log(`rr-save: stored ${path} (${len} bytes)`),
      (e) => console.error(`rr-save: failed to store ${path}: ${e}`),
    );
    return 1;
  }
}

/// Programs to run, chosen with `?exe=`.
// TODO: we'll put program params in the values of this map.
const PROGRAMS = new Map([
  ["mine", {}],
  ["basicdd", {}],
  ["winapi", {}],
  ["roadrash", {}],
]);

async function main() {
  if (!window.SharedArrayBuffer) {
    document.body.innerText = "SharedArrayBuffer is not supported; possibly try reloading";
    return;
  }

  const wasmPageSize = 64 << 10;
  const memory = new WebAssembly.Memory({
    // memory args are in units of wasm page size
    initial: (16 << 20) / wasmPageSize, // Road Rash evaluation: was 2 MB, too small
    maximum: (1024 << 20) / wasmPageSize, // Road Rash evaluation: was 512 MB
    shared: true,
  });

  // Road Rash evaluation: the GitHub Pages copy is encrypted; ask for the password.
  const key = await unlock();
  showProgress({ label: "Loading the game engine" });
  const host = new Host(memory);
  const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  // Road Rash evaluation: thread spawn requests from the program worker.
  const onMsg = (e: MessageEvent) => {
    if (e.data && e.data.rrFetch) {
      fileFetch(e.data.rrFetch.path, e.data.rrFetch.busy);
      return;
    }
    if (e.data && e.data.rrProgress) {
      showProgress(e.data.rrProgress);
      // Once the game runs, fetch the rest in the background.
      if (e.data.rrProgress.done) startPrefetch(DIR).catch((err) => console.warn("rr-prefetch", err));
      return;
    }
    if (e.data && e.data.rrSpawn) {
      const { name, module, ptr, lazyNames, key, dir } = e.data.rrSpawn;
      const t = new Worker(new URL("./thread-worker.ts", import.meta.url), { type: "module" });
      t.onmessage = onMsg;
      t.postMessage({ name, module, memory, ptr, lazyNames, key, dir });
      console.log(`rr: spawned thread worker for closure ${ptr}`);
      return;
    }
    host.onMessage(e);
  };
  worker.onmessage = onMsg;

  const params = new URLSearchParams(window.location.search);
  const name = params.get("exe") ?? "roadrash";
  const program = PROGRAMS.get(name);
  if (!program) {
    document.body.innerText = `no such program ${name}`;
    return;
  }

  const message: worker.StartMessage = {
    ...program,
    name,
    memory,
    query: window.location.search,
    key,
    dir: DIR,
  };
  worker.postMessage(message);
}
main().catch((e) => console.error(e));
