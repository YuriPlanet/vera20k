// The dispatch key of a named action, as `actionKey` in
// assembly/triggers.ts computes it at run time: FNV-1a over the
// ASCII-lowercased name, bit 30 set and bit 31 clear.
export function actionKey(name) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < name.length; i++) {
    let c = name.charCodeAt(i);
    if (c >= 65 && c <= 90) c += 32;
    hash = Math.imul(hash ^ (c & 0xff), 0x01000193) >>> 0;
  }
  return ((hash & 0x3fffffff) | 0x40000000) | 0;
}
