/**
 * Friendly display names for transcription model IDs.
 *
 * The canonical source is models/manifest.json (fetched at runtime via
 * fetch_model_manifest). This static map covers the IDs that appear in
 * historical transcription records and the insights dashboard — the manifest
 * is not available at render time for those records.
 *
 * Fallback: stripModelId() cleans up any unknown ID into a readable label.
 */

const MODEL_DISPLAY_NAMES: Record<string, string> = {
  // Whisper ggml models (from manifest.json)
  'ggml-large-v3-turbo': 'Whisper Large v3 Turbo',
  'ggml-medium.en': 'Whisper Medium (English)',
  'ggml-small.en': 'Whisper Small (English)',
  // Parakeet sherpa-rs models
  'parakeet-tdt-0.6b-v2-int8': 'Parakeet TDT v2',
  'parakeet-tdt-0.6b-v3-int8': 'Parakeet TDT v3',
  // FluidAudio CoreML model
  'fluidaudio-parakeet-tdt-coreml': 'Parakeet TDT v3 (ANE)',
  // Legacy / alternate spellings that may appear in old records
  'parakeet-tdt-1.1': 'Parakeet TDT v1.1',
  fluidaudio: 'Parakeet TDT v3 (ANE)',
  whisper: 'Whisper',
  parakeet: 'Parakeet TDT',
};

/**
 * Produce a readable label for a model ID that is not in the map.
 * Strips vendor prefixes, file extensions, and cleans up separators.
 */
function stripModelId(id: string): string {
  return (
    id
      // drop ggml- prefix
      .replace(/^ggml-/i, '')
      // drop .bin extension
      .replace(/\.bin$/i, '')
      // drop fluidaudio- prefix
      .replace(/^fluidaudio-/i, '')
      // normalise separators to spaces
      .replace(/[-_.]/g, ' ')
      // title-case each word
      .split(' ')
      .filter(Boolean)
      .map((w) => {
        // Keep known abbreviations uppercase
        if (/^(tdt|ane|coreml|v\d|int\d|en|gpu)$/i.test(w)) {
          // Preserve "v3", "int8" casing; uppercase short all-alpha tokens
          if (/^v\d/i.test(w)) return w.toLowerCase();
          if (/^int\d/i.test(w)) return w.toLowerCase();
          return w.toUpperCase();
        }
        return w.charAt(0).toUpperCase() + w.slice(1).toLowerCase();
      })
      .join(' ')
  );
}

/** Return a human-friendly display name for a transcription model ID. */
export function friendlyModelName(id: string): string {
  if (!id) return 'Unknown';
  const direct = MODEL_DISPLAY_NAMES[id];
  if (direct) return direct;
  // Case-insensitive lookup
  const lower = id.toLowerCase();
  const caseInsensitive = Object.entries(MODEL_DISPLAY_NAMES).find(
    ([k]) => k.toLowerCase() === lower
  );
  if (caseInsensitive) return caseInsensitive[1];
  return stripModelId(id);
}
