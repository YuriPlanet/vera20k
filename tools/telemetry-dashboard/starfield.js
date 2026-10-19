/**
 * Starfield backdrop for the debug wall.
 *
 * The panels sit on a live perspective star field rather than a static gradient:
 * every star carries a depth and a spectral class, so its colour comes from the
 * class and its size, brightness and speed from the depth. The trail is one
 * frame's screen displacement, which is why a slow crawl reads as points and
 * only real speed smears into streaks — a fixed shutter length turned every
 * star into a visible line segment.
 *
 * Presentation only: nothing here feeds the simulation.
 */
/**
 * Spectral classes, blue-white through to red. Red dwarfs are the commonest
 * stars there are but far too faint to see, so these weights follow the
 * naked-eye sky — B supergiants and K/M giants — which is what makes the field
 * read as a blue-to-amber spread instead of one flat white.
 */
const SPECTRA = [
    { rgb: '155,178,255', weight: 0.010 },
    { rgb: '170,196,255', weight: 0.075 },
    { rgb: '202,219,255', weight: 0.130 },
    { rgb: '246,246,255', weight: 0.185 },
    { rgb: '255,244,224', weight: 0.200 },
    { rgb: '255,206,150', weight: 0.240 },
    { rgb: '255,150,104', weight: 0.160 },
];
const STAR_COUNT = 1200;
/** Depth units per second. Calm on purpose: the wall is read for hours. */
const WARP = 0.12;
/** Trail length as a multiple of one frame's displacement. */
const TRAIL_FRAMES = 1.3;
/** Focal length as a fraction of the shorter side. Narrower crops the field so
 *  the near, bright stars leave the frame before they are large. */
const FOCAL = 0.62;
const MAX_DPR = 2;
const SPRITE = 56;
const BOX_X = 2.4;
const BOX_Y = 1.25;
const MIN_Z = 0.055;
/** How far the vanishing point may drift from the centre under the pointer. */
const LOOK_RANGE = 0.24;
function bakeSprites() {
    return SPECTRA.map((spectrum) => {
        const sprite = document.createElement('canvas');
        sprite.width = SPRITE;
        sprite.height = SPRITE;
        const g = sprite.getContext('2d');
        if (!g)
            return sprite;
        const half = SPRITE / 2;
        const grad = g.createRadialGradient(half, half, 0, half, half, half);
        grad.addColorStop(0, 'rgba(255,255,255,1)');
        grad.addColorStop(0.055, 'rgba(255,255,255,0.92)');
        grad.addColorStop(0.11, `rgba(${spectrum.rgb},0.8)`);
        grad.addColorStop(0.26, `rgba(${spectrum.rgb},0.26)`);
        grad.addColorStop(0.55, `rgba(${spectrum.rgb},0.07)`);
        grad.addColorStop(1, `rgba(${spectrum.rgb},0)`);
        g.fillStyle = grad;
        g.fillRect(0, 0, SPRITE, SPRITE);
        return sprite;
    });
}
export function createStarfield(canvas) {
    const ctx = canvas.getContext('2d');
    if (!ctx)
        return null;
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    const sprites = bakeSprites();
    // Seeded so the same machine gets the same sky across reloads.
    let seed = 0x5ea17c01;
    const next = () => {
        seed = (seed * 1103515245 + 12345) & 0x7fffffff;
        return (seed >>> 9) / 4194304;
    };
    const cumulative = [];
    let totalWeight = 0;
    for (const spectrum of SPECTRA) {
        totalWeight += spectrum.weight;
        cumulative.push(totalWeight);
    }
    const pickClass = () => {
        const draw = next() * totalWeight;
        const index = cumulative.findIndex((cum) => draw <= cum);
        return index < 0 ? cumulative.length - 1 : index;
    };
    const stars = [];
    const reset = (star) => {
        star.x = (next() * 2 - 1) * BOX_X;
        star.y = (next() * 2 - 1) * BOX_Y;
        star.z = MIN_Z + next() * (1 - MIN_Z);
        star.m = Math.pow(next(), 2.3);
        star.phase = next() * Math.PI * 2;
        star.cls = pickClass();
    };
    for (let i = 0; i < STAR_COUNT; i++) {
        const star = { x: 0, y: 0, z: 1, m: 0, phase: 0, cls: 0 };
        reset(star);
        stars.push(star);
    }
    let width = 0;
    let height = 0;
    let elapsed = 0;
    let centreX = 0.5;
    let centreY = 0.5;
    let aimX = 0.5;
    let aimY = 0.5;
    const resize = () => {
        const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
        width = canvas.clientWidth;
        height = canvas.clientHeight;
        canvas.width = Math.max(1, Math.round(width * dpr));
        canvas.height = Math.max(1, Math.round(height * dpr));
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    const draw = (dt) => {
        const focal = Math.min(width, height) * FOCAL;
        const ox = centreX * width;
        const oy = centreY * height;
        const warp = reduceMotion ? 0 : WARP;
        ctx.clearRect(0, 0, width, height);
        // Additive: where the field crowds together it should bloom, not occlude.
        ctx.globalCompositeOperation = 'lighter';
        ctx.lineCap = 'round';
        for (const star of stars) {
            star.z -= dt * warp;
            if (star.z < MIN_Z) {
                reset(star);
                continue;
            }
            const k = focal / star.z;
            const sx = ox + star.x * k;
            const sy = oy + star.y * k;
            if (sx < -320 || sx > width + 320 || sy < -320 || sy > height + 320)
                continue;
            const depth = 1 - star.z;
            const d2 = depth * depth;
            const twinkle = 0.86 + 0.14 * Math.sin(elapsed * 2.1 + star.phase);
            const shift = (focal * warp * (dt * TRAIL_FRAMES)) / Math.max(0.0016, star.z * star.z);
            const dx = star.x * shift;
            const dy = star.y * shift;
            const raw = Math.hypot(dx, dy);
            const length = Math.min(340, raw);
            const rgb = SPECTRA[star.cls].rgb;
            const alpha = Math.min(1, (0.2 + d2 * 1.1) * twinkle * (0.3 + star.m) + 0.16);
            if (length > 2.2) {
                const scale = length / raw;
                ctx.strokeStyle = `rgba(${rgb},${(alpha * 0.42).toFixed(3)})`;
                ctx.lineWidth = 0.45 + d2 * 1.35;
                ctx.beginPath();
                ctx.moveTo(sx - dx * scale, sy - dy * scale);
                ctx.lineTo(sx, sy);
                ctx.stroke();
            }
            const size = (3.5 + d2 * 15 + star.m * 18) * twinkle;
            ctx.globalAlpha = Math.min(1, alpha * 1.3);
            ctx.drawImage(sprites[star.cls], sx - size / 2, sy - size / 2, size, size);
            ctx.globalAlpha = 1;
            if (star.m > 0.9 && d2 > 0.35) {
                const arm = size * 0.6;
                ctx.strokeStyle = `rgba(${rgb},${(alpha * 0.3).toFixed(3)})`;
                ctx.lineWidth = 0.8;
                ctx.beginPath();
                ctx.moveTo(sx - arm, sy);
                ctx.lineTo(sx + arm, sy);
                ctx.moveTo(sx, sy - arm);
                ctx.lineTo(sx, sy + arm);
                ctx.stroke();
            }
        }
        ctx.globalCompositeOperation = 'source-over';
    };
    let raf = 0;
    let last = 0;
    const frame = (now) => {
        raf = requestAnimationFrame(frame);
        const dt = last ? Math.min(0.05, (now - last) / 1000) : 1 / 60;
        last = now;
        elapsed += dt;
        centreX += (aimX - centreX) * Math.min(1, dt * 2.5);
        centreY += (aimY - centreY) * Math.min(1, dt * 2.5);
        draw(dt);
    };
    const start = () => {
        if (raf)
            return;
        last = 0;
        raf = requestAnimationFrame(frame);
    };
    const stop = () => {
        if (!raf)
            return;
        cancelAnimationFrame(raf);
        raf = 0;
    };
    // A background tab that keeps painting costs the machine for no reader, so
    // the loop is parked until the wall is visible again.
    const onVisibility = () => (document.hidden ? stop() : start());
    // The vanishing point drifts within a band around the centre rather than
    // landing under the cursor: chasing the mouse across a wall read for hours
    // is disorienting, and a small parallax still sells the depth.
    const onPointer = (event) => {
        aimX = 0.5 + (event.clientX / window.innerWidth - 0.5) * LOOK_RANGE;
        aimY = 0.5 + (event.clientY / window.innerHeight - 0.5) * LOOK_RANGE;
    };
    resize();
    window.addEventListener('resize', resize);
    if (reduceMotion) {
        draw(0);
    }
    else {
        window.addEventListener('pointermove', onPointer);
        document.addEventListener('visibilitychange', onVisibility);
        // The field starts uniform in depth, but warming it up means the very first
        // painted frame already has stars that have travelled.
        for (let i = 0; i < 120; i++)
            draw(1 / 60);
        start();
    }
    return {
        dispose() {
            stop();
            window.removeEventListener('resize', resize);
            window.removeEventListener('pointermove', onPointer);
            document.removeEventListener('visibilitychange', onVisibility);
        },
    };
}
const target = document.getElementById('starfield');
if (target instanceof HTMLCanvasElement)
    createStarfield(target);
