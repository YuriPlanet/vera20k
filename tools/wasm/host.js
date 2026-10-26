import init, { import_file, prepare_audio, restore_user_file, set_persistence_callbacks, start_game } from './pkg/vera20k.js';

const status = document.querySelector('#status');
const storageStatus = document.querySelector('#storage');
const picker = document.querySelector('#files');
const start = document.querySelector('#start');
const canvas = document.querySelector('#game');
let database;
let storageFailed = false;
let pending = 0;
let saveQueue = Promise.resolve();
const show = message => { status.textContent = message; };
const storageError = error => {
    storageFailed = true;
    storageStatus.textContent = `User data could not be saved in this browser: ${error.message || error}`;
};
function storageTransaction(operation) {
    return new Promise((resolve, reject) => {
        const tx = database.transaction('files', 'readwrite');
        operation(tx.objectStore('files'));
        tx.oncomplete = resolve;
        tx.onerror = tx.onabort = () => reject(tx.error || new Error('Storage transaction aborted'));
    });
}
function persist(operation) {
    if (!database) { storageError(new Error('Storage unavailable')); return; }
    pending++;
    storageStatus.textContent = 'Saving user data…';
    saveQueue = saveQueue.then(() => storageTransaction(operation)).catch(storageError).finally(() => {
        pending--;
        if (!pending && !storageFailed) storageStatus.textContent = 'User data saved in this browser.';
    });
}
async function restore() {
    database = await new Promise((resolve, reject) => {
        const request = indexedDB.open('vera20k-wasm-user-data', 1);
        request.onupgradeneeded = () => request.result.createObjectStore('files', { keyPath: 'path' });
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
        request.onblocked = () => reject(new Error('Close other game tabs to open storage'));
    });
    const records = await new Promise((resolve, reject) => {
        const request = database.transaction('files').objectStore('files').getAll();
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
    });
    for (const { path, bytes, modified } of records) restore_user_file(path, new Uint8Array(bytes), modified);
}

try {
    await init();
    try { await restore(); } catch (error) { storageError(error); }
    set_persistence_callbacks((path, bytes, modified) => {
        // Copy before returning: the source may refer to transient Wasm memory.
        const copy = bytes.slice();
        persist(store => store.put({ path, bytes: copy, modified }));
    }, path => persist(store => store.delete(path)));
    show('Select a game folder to begin.');
    picker.addEventListener('change', () => { start.disabled = !picker.files.length; });
    start.disabled = !picker.files.length;
    start.addEventListener('click', async () => {
        start.disabled = true;
        picker.disabled = true;
        try {
            try { prepare_audio(); } catch (error) { show(`Audio unavailable: ${error}`); }
            let count = 0;
            for (const file of picker.files) {
                const relative = file.webkitRelativePath.split('/').slice(1).join('/');
                // Ignore executable installers and development metadata. The same formats
                // may also appear in nested game resource folders.
                if (!/\.(mix|mmx|yrm|map|ini|pkt|csf|fnt|wav|aud|shp|pal|vxl|hva|tmp|pcx|bik)$/i.test(relative)) continue;
                show(`Importing ${++count}: ${relative}`);
                import_file(relative, new Uint8Array(await file.arrayBuffer()));
            }
            start_game(canvas, show);
            canvas.focus();
        } catch (error) {
            show(`Could not start: ${error}. Reload to try again.`);
        }
    });
} catch (error) {
    show(`Could not load the engine: ${error}`);
}
window.addEventListener('beforeunload', event => {
    if (pending || storageFailed) { event.preventDefault(); event.returnValue = ''; }
});
