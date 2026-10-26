/** Reads the current key, migrating an earlier branded preference when available. */
export function readPreference(key: string): string | null {
  const current = localStorage.getItem(key);
  if (current !== null) return current;
  const legacyKey = key.replace(/^pcbplannercad\./, "allroute.");
  const legacy = localStorage.getItem(legacyKey);
  if (legacy !== null) {
    localStorage.setItem(key, legacy);
    localStorage.removeItem(legacyKey);
  }
  return legacy;
}
