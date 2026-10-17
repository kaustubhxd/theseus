// Road Rash evaluation: files the program writes (save games, registry) are
// kept in IndexedDB so they survive page reloads. Used by both the page (to
// store) and the program worker (to restore at start-up).

const DB = "theseus-roadrash";
const STORE = "files";

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export async function saveFile(path: string, data: Uint8Array): Promise<void> {
  const db = await open();
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(data, path);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
  db.close();
}

export async function loadAll(): Promise<[string, Uint8Array][]> {
  const db = await open();
  const out: [string, Uint8Array][] = await new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readonly");
    const store = tx.objectStore(STORE);
    const keys = store.getAllKeys();
    const vals = store.getAll();
    tx.oncomplete = () => resolve((keys.result as string[]).map((k, i) => [k, vals.result[i] as Uint8Array]));
    tx.onerror = () => reject(tx.error);
  });
  db.close();
  return out;
}

export async function clearAll(): Promise<void> {
  const db = await open();
  await new Promise<void>((resolve) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).clear();
    tx.oncomplete = () => resolve();
  });
  db.close();
}
