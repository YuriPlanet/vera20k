const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const {resolve} = require('node:path');
const {test} = require('node:test');

const root = resolve(__dirname, '../../..');
const helper = readFileSync(resolve(root, 'src/web/screenshot.js'));
const modulePromise = import('data:text/javascript;base64,' + helper.toString('base64'));

test('requests a canvas PNG immediately and downloads it using a temporary URL', async () => {
    const {downloadCanvasScreenshot} = await modulePromise;
    const events = [];
    const blob = new Blob(['png'], {type: 'image/png'});
    const link = {
        click() {
            assert.equal(this.href, 'blob:frame');
            assert.match(this.download, /^vera20k-.*\.png$/);
            events.push('click');
        },
        remove() { events.push('remove'); },
    };
    let encode, expire;
    const canvas = {
        ownerDocument: {
            defaultView: {
                URL: {
                    createObjectURL(value) {
                        assert.equal(value, blob);
                        return 'blob:frame';
                    },
                    revokeObjectURL(value) { events.push(['revoke', value]); },
                },
                setTimeout(callback) { expire = callback; },
            },
            body: {appendChild(value) { assert.equal(value, link); }},
            createElement(type) { assert.equal(type, 'a'); return link; },
        },
        toBlob(callback, mime) {
            assert.equal(mime, 'image/png');
            encode = callback;
            events.push('snapshot');
        },
    };
    downloadCanvasScreenshot(canvas);
    assert.deepEqual(events, ['snapshot']);
    encode(blob);
    assert.deepEqual(events, ['snapshot', 'click', 'remove']);
    expire();
    assert.deepEqual(events.at(-1), ['revoke', 'blob:frame']);
});

test('failed canvas capture produces no download and does not interrupt a frame', async t => {
    const {downloadCanvasScreenshot} = await modulePromise;
    const errors = t.mock.method(console, 'error', () => {});
    const detached = {ownerDocument: {defaultView: {}}};
    assert.doesNotThrow(() => downloadCanvasScreenshot({
        ...detached,
        toBlob(callback) { callback(null); },
    }));
    assert.doesNotThrow(() => downloadCanvasScreenshot({
        ...detached,
        toBlob() { throw new Error('capture unavailable'); },
    }));
    assert.equal(errors.mock.callCount(), 2);
});

test('screenshot request is consumed once and takes the canvas path after web presentation', () => {
    const frame = readFileSync(resolve(root, 'src/app/frame.rs'), 'utf8');
    const consume = frame.indexOf('std::mem::take(&mut state.match_state.input.retail_screenshot_requested)');
    const handoff = frame.indexOf('let submission = state.renderer.gpu.hand_off_frame(encoder, output)', consume);
    const screenshot = frame.indexOf('crate::web::download_screenshot();', handoff);
    assert(consume >= 0 && handoff > consume && screenshot > handoff);
    assert.match(frame.slice(consume, handoff), /screenshot_requested\s*&& !cfg!/);
    assert.match(frame.slice(handoff, screenshot), /#\[cfg\(all\(feature = "wasm", target_arch = "wasm32", target_os = "unknown"\)\)\]/);
});
