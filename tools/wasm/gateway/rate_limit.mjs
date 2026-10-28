// Fixed one-second window shared by the relay transport and LAN signaling.
export function rateLimit(maximum) {
  let start = performance.now();
  let count = 0;
  return () => {
    const now = performance.now();
    if (now - start >= 1000) { start = now; count = 0; }
    return ++count <= maximum;
  };
}
