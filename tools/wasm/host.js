import init, { import_file, prepare_audio, restore_user_file, set_persistence_callbacks, start_game } from './pkg/vera20k.js';

const $ = selector => document.querySelector(selector);
const status = $('#status');
const storageStatus = $('#storage');
const picker = $('#files');
const pick = $('#pick');
const drop = $('#drop');
const start = $('#start');
const canvas = $('#game');
const launcher = $('#launcher');
const hud = $('#hud');
const progress = $('#progress');
const folderDetail = $('#folder-detail');
const fileTotal = $('#file-total');
const steps = { browser: $('#step-browser'), folder: $('#step-folder'), start: $('#step-start') };
// Ignore executable installers and development metadata. The same formats
// may also appear in nested game resource folders.
const RESOURCE = /\.(mix|mmx|yrm|map|ini|pkt|csf|fnt|wav|aud|shp|pal|vxl|hva|tmp|pcx|bik)$/i;

const TEXT = {
    en: {
        tagline: 'Yuri’s Revenge · Web edition',
        browserTitle: 'Check browser', folderTitle: 'Import game files', startTitle: 'Start game',
        dropHint: 'Drop your game folder here', dropSub: 'or click to choose it',
        manifest: 'GAME FILES', filesLabel: 'Files', present: 'found', absent: 'missing',
        pick: 'Choose game folder', start: 'Start game',
        engineLoading: 'Loading engine…',
        browserReady: 'Engine ready. Choose your Yuri’s Revenge folder.',
        insecure: 'Open this page over HTTPS or from localhost; WebGPU needs a secure page.',
        noGpu: 'This browser has no WebGPU. Use a recent Chrome or Edge.',
        engineFailed: error => `Could not load the engine: ${error}`,
        reading: 'Reading folder…',
        found: folder => `Using the game folder inside “${folder}”.`,
        noFiles: 'No game files found in that folder. Pick the Yuri’s Revenge install folder.',
        notYr: 'ra2md.mix was not found; this may not be a Yuri’s Revenge folder.',
        startReady: 'Ready. Click Start game.',
        importing: (n, total, name) => `Importing ${n}/${total}: ${name}`,
        startFailed: error => `Could not start: ${error}. Reload the page to try again.`,
        helpTitle: 'Help', helpHeading: 'Where is my game folder?',
        help1: 'Pick the Yuri’s Revenge install folder: the one that contains <code>ra2md.mix</code> and <code>ra2.mix</code>.',
        help2: 'Steam / EA app installs usually live under <code>Command and Conquer Red Alert II</code> in the library folder.',
        help3: 'Choosing a parent folder is fine — the game folder inside it is found automatically.',
        help4: 'Nothing is uploaded. Game files are read into memory and must be chosen again after reloading.',
        help5: 'Needs WebGPU: a recent Chrome or Edge works best. Large installs need plenty of memory.',
        storage: 'Settings and saves are stored in this browser for this site.',
        saving: 'Saving user data…', saved: 'User data saved in this browser.',
        storageFailed: error => `User data could not be saved in this browser: ${error}`,
        language: '中文',
    },
    zh: {
        tagline: '尤里的复仇 · 网页版',
        browserTitle: '检查浏览器', folderTitle: '导入游戏资源', startTitle: '开始游戏',
        dropHint: '把游戏文件夹拖到这里', dropSub: '或点击此处选择',
        manifest: '游戏资源', filesLabel: '文件', present: '已找到', absent: '缺失',
        pick: '选择游戏文件夹', start: '开始游戏',
        engineLoading: '正在加载引擎…',
        browserReady: '引擎已就绪，请选择《尤里的复仇》游戏文件夹',
        insecure: '请通过 HTTPS 或 localhost 打开本页；WebGPU 需要安全页面。',
        noGpu: '当前浏览器不支持 WebGPU，请使用新版 Chrome 或 Edge。',
        engineFailed: error => `引擎加载失败：${error}`,
        reading: '正在读取文件夹…',
        found: folder => `已使用“${folder}”中的游戏目录。`,
        noFiles: '该文件夹中没有游戏文件，请选择《尤里的复仇》安装目录。',
        notYr: '未找到 ra2md.mix，这可能不是《尤里的复仇》目录。',
        startReady: '准备就绪，点击“开始游戏”',
        importing: (n, total, name) => `正在导入 ${n}/${total}：${name}`,
        startFailed: error => `无法启动：${error}。请刷新页面重试。`,
        helpTitle: '帮助', helpHeading: '游戏文件夹在哪里？',
        help1: '选择《尤里的复仇》安装目录，即包含 <code>ra2md.mix</code> 和 <code>ra2.mix</code> 的文件夹。',
        help2: 'Steam / EA app 版本通常位于游戏库中的 <code>Command and Conquer Red Alert II</code> 目录。',
        help3: '选择上一级文件夹也可以，会自动找到其中的游戏目录。',
        help4: '不会上传任何文件。游戏文件只读入内存，刷新页面后需要重新选择。',
        help5: '需要 WebGPU，推荐使用新版 Chrome 或 Edge。完整安装需要较多内存。',
        storage: '设置和存档保存在此浏览器中（仅限本站）。',
        saving: '正在保存用户数据…', saved: '用户数据已保存到此浏览器。',
        storageFailed: error => `无法在此浏览器中保存用户数据：${error}`,
        language: 'English',
        modeSingle: '单人游戏。资源只在本机读取，不会上传。',
        modeOnline: '联机时可能共享所选地图，其它资源只在本机读取。',
    },
};
let lang = 'en';
try { lang = localStorage.getItem('vera20k-lang') || ''; } catch {}
if (!TEXT[lang]) lang = navigator.language?.toLowerCase().startsWith('zh') ? 'zh' : 'en';
// The packager writes the English mode line for this build; translations follow it.
const modeEnglish = $('#mode').textContent;
const online = typeof globalThis.veraSocketOpen === 'function';
// Dynamic text, re-rendered when the language changes.
const live = new Map();
function render(element, key, args) {
    const value = TEXT[lang][key];
    element.textContent = typeof value === 'function' ? value(...args) : value;
}
function say(element, key, ...args) {
    live.set(element, [key, args]);
    render(element, key, args);
}
function applyLanguage() {
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';
    for (const element of document.querySelectorAll('[data-i18n]')) element.textContent = TEXT[lang][element.dataset.i18n];
    for (const element of document.querySelectorAll('[data-i18n-html]')) element.innerHTML = TEXT[lang][element.dataset.i18nHtml];
    $('#mode').textContent = lang === 'en' ? modeEnglish : TEXT.zh[online ? 'modeOnline' : 'modeSingle'];
    $('#lang').textContent = TEXT[lang].language;
    for (const [element, [key, args]] of live) render(element, key, args);
}
$('#lang').addEventListener('click', () => {
    lang = lang === 'en' ? 'zh' : 'en';
    try { localStorage.setItem('vera20k-lang', lang); } catch {}
    applyLanguage();
});
$('#help-toggle').addEventListener('click', event => {
    const help = $('#help');
    help.hidden = !help.hidden;
    event.currentTarget.setAttribute('aria-expanded', String(!help.hidden));
});
applyLanguage();

function setStatus(tone, key, ...args) {
    status.dataset.tone = tone;
    say(status, key, ...args);
}
function setFolder(tone, key, ...args) {
    folderDetail.className = tone;
    if (key) say(folderDetail, key, ...args);
    else { live.delete(folderDetail); folderDetail.textContent = ''; }
}
const setStep = (step, state) => { step.dataset.state = state; };
// `top` is the set of lower-case file names directly in the game folder, or null when unknown.
function showManifest(top, total = '—') {
    for (const cell of document.querySelectorAll('#checks [data-file]')) {
        if (!top) { live.delete(cell); cell.className = 'idle'; cell.textContent = '—'; continue; }
        const present = top.has(cell.dataset.file);
        cell.className = present ? 'ok' : 'missing';
        say(cell, present ? 'present' : 'absent');
    }
    fileTotal.className = top ? '' : 'idle';
    fileTotal.textContent = total;
}
// The red plate always marks the next thing to do.
function ready(canStart) {
    start.disabled = !canStart;
    start.classList.toggle('primary', canStart);
    pick.classList.toggle('primary', !canStart);
    pick.classList.toggle('secondary', canStart);
}

let started = false;
let hudTimer;
const show = message => {
    if (!started) {
        live.delete(status);
        status.textContent = message;
        return;
    }
    hud.hidden = false;
    hud.classList.remove('fade');
    hud.textContent = message;
    clearTimeout(hudTimer);
    hudTimer = setTimeout(() => hud.classList.add('fade'), 5000);
};

let database;
let storageFailed = false;
let pending = 0;
let saveQueue = Promise.resolve();
const storageError = error => {
    storageFailed = true;
    say(storageStatus, 'storageFailed', error.message || String(error));
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
    say(storageStatus, 'saving');
    saveQueue = saveQueue.then(() => storageTransaction(operation)).catch(storageError).finally(() => {
        pending--;
        if (!pending && !storageFailed) say(storageStatus, 'saved');
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

// Resources to import, as [path below the game folder, File].
let selection = [];
const formatSize = bytes => bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`;

// `entries` hold paths relative to what the player picked or dropped. The game folder is
// the shallowest one holding ra2md.mix, so a parent folder works too.
function choose(entries) {
    const depth = path => path.split('/').length;
    const key = entries.filter(([path]) => /(^|\/)ra2md\.mix$/i.test(path)).sort((a, b) => depth(a[0]) - depth(b[0]))[0];
    let root;
    if (key) root = key[0].slice(0, key[0].length - 'ra2md.mix'.length);
    else {
        // Without a marker, drop the picked folder's own name as before.
        const first = entries[0]?.[0].split('/')[0];
        root = entries.length && entries.every(([path]) => path.startsWith(`${first}/`)) ? `${first}/` : '';
    }
    selection = entries
        .filter(([path]) => path.startsWith(root))
        .map(([path, file]) => [path.slice(root.length), file])
        .filter(([path]) => RESOURCE.test(path));

    if (!selection.length) {
        showManifest(null);
        setFolder('err', 'noFiles');
        setStatus('err', 'noFiles');
        setStep(steps.folder, 'error');
        setStep(steps.start, 'waiting');
        ready(false);
        return;
    }
    const top = new Set(selection.filter(([path]) => !path.includes('/')).map(([path]) => path.toLowerCase()));
    const bytes = selection.reduce((sum, [, file]) => sum + file.size, 0);
    showManifest(top, `${selection.length} · ${formatSize(bytes)}`);
    const subfolder = root.replace(/\/$/, '');
    const nested = key && subfolder && subfolder !== entries[0][0].split('/')[0] ? subfolder : '';
    if (!key) setFolder('warn', 'notYr');
    else if (nested) setFolder('', 'found', nested);
    else setFolder('');
    setStatus('ok', 'startReady');
    setStep(steps.folder, key ? 'done' : 'warn');
    setStep(steps.start, 'active');
    ready(true);
    start.focus();
}

async function dropped(items) {
    const out = [];
    async function walk(entry) {
        if (entry.isFile) {
            out.push([entry.fullPath.replace(/^\//, ''), await new Promise((resolve, reject) => entry.file(resolve, reject))]);
            return;
        }
        const reader = entry.createReader();
        for (;;) {
            const batch = await new Promise((resolve, reject) => reader.readEntries(resolve, reject));
            if (!batch.length) break;
            for (const child of batch) await walk(child);
        }
    }
    for (const entry of items) await walk(entry);
    return out;
}

async function launch() {
    start.disabled = true;
    pick.disabled = true;
    picker.disabled = true;
    drop.setAttribute('aria-disabled', 'true');
    try {
        try { prepare_audio(); } catch (error) { show(`Audio unavailable: ${error}`); }
        progress.hidden = false;
        const bar = progress.firstElementChild;
        const total = selection.reduce((sum, [, file]) => sum + file.size, 0) || 1;
        let done = 0;
        for (const [index, [path, file]] of selection.entries()) {
            setStatus('busy', 'importing', index + 1, selection.length, path);
            import_file(path, new Uint8Array(await file.arrayBuffer()));
            done += file.size;
            bar.style.width = `${(100 * done / total).toFixed(1)}%`;
        }
        start_game(canvas, show);
        started = true;
        launcher.classList.add('leaving');
        setTimeout(() => { launcher.hidden = true; }, 400);
        canvas.focus();
    } catch (error) {
        setStep(steps.start, 'error');
        setStatus('err', 'startFailed', String(error));
    }
}

try {
    await init();
    try { await restore(); } catch (error) { storageError(error); }
    set_persistence_callbacks((path, bytes, modified) => {
        // Copy before returning: the source may refer to transient Wasm memory.
        const copy = bytes.slice();
        persist(store => store.put({ path, bytes: copy, modified }));
    }, path => persist(store => store.delete(path)));

    if (!isSecureContext || !navigator.gpu) {
        setStep(steps.browser, 'error');
        setStatus('err', isSecureContext ? 'noGpu' : 'insecure');
    } else {
        setStep(steps.browser, 'done');
        setStep(steps.folder, 'active');
        setStatus('ok', 'browserReady');
        pick.disabled = false;
        picker.disabled = false;
        drop.setAttribute('aria-disabled', 'false');
        const browse = () => { if (!picker.disabled) picker.click(); };
        pick.addEventListener('click', browse);
        drop.addEventListener('click', browse);
        drop.addEventListener('keydown', event => {
            if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); browse(); }
        });
        picker.addEventListener('change', () => {
            choose([...picker.files].map(file => [file.webkitRelativePath || file.name, file]));
        });
        // A drop outside the zone would otherwise make the browser open the file.
        for (const type of ['dragover', 'drop']) document.addEventListener(type, event => event.preventDefault());
        drop.addEventListener('dragover', () => drop.classList.add('over'));
        drop.addEventListener('dragleave', event => {
            if (!drop.contains(event.relatedTarget)) drop.classList.remove('over');
        });
        drop.addEventListener('drop', async event => {
            drop.classList.remove('over');
            if (picker.disabled) return;
            // Entries must be taken before the first await; the transfer expires afterwards.
            const items = [...event.dataTransfer.items].map(item => item.webkitGetAsEntry?.()).filter(Boolean);
            setFolder('', 'reading');
            try { choose(await dropped(items)); }
            catch (error) { folderDetail.className = 'err'; folderDetail.textContent = String(error); }
        });
        start.addEventListener('click', launch);
    }
} catch (error) {
    setStep(steps.browser, 'error');
    setStatus('err', 'engineFailed', String(error));
}
window.addEventListener('beforeunload', event => {
    if (pending || storageFailed) { event.preventDefault(); event.returnValue = ''; }
});
