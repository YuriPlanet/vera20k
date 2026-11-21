// Snapshot the canvas in the same render turn as presentation. Moving toBlob
// itself to a later callback can capture an already-discarded WebGL buffer.
export function downloadCanvasScreenshot(canvas) {
    const failed = error => console.error("Screenshot download failed:", error);
    try {
        const document = canvas.ownerDocument;
        const window = document.defaultView;
        const fileName = `vera20k-${new Date().toISOString().replaceAll(":", "-")}.png`;
        canvas.toBlob(blob => {
            if (!blob) {
                failed("canvas PNG encoding returned no image");
                return;
            }
            let url;
            let link;
            try {
                url = window.URL.createObjectURL(blob);
                link = document.createElement("a");
                link.href = url;
                link.download = fileName;
                link.hidden = true;
                document.body.appendChild(link);
                link.click();
            } catch (error) {
                failed(error);
            } finally {
                link?.remove();
                if (url) window.setTimeout(() => window.URL.revokeObjectURL(url), 1000);
            }
        }, "image/png");
    } catch (error) {
        failed(error);
    }
}
