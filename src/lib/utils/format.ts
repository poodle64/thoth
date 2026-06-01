/**
 * Shared formatting utilities for displaying durations, speeds, and timestamps.
 */

/** Format a duration in seconds to a compact string (e.g. "3.2s", "2m 15s") */
export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  const mins = Math.floor(seconds / 60);
  const secs = Math.round(seconds % 60);
  return `${mins}m ${secs}s`;
}

/** Format a total duration with hours support (e.g. "45s", "12m 30s", "2h 15m") */
export function formatTotalDuration(seconds: number): string {
  if (seconds < 60) return `${seconds.toFixed(0)}s`;
  if (seconds < 3600) {
    const mins = Math.floor(seconds / 60);
    const secs = Math.round(seconds % 60);
    return `${mins}m ${secs}s`;
  }
  const hours = Math.floor(seconds / 3600);
  const mins = Math.round((seconds % 3600) / 60);
  return `${hours}h ${mins}m`;
}

/** Format a real-time speed factor (e.g. "8.2x") */
export function formatSpeedFactor(factor: number): string {
  return `${factor.toFixed(1)}x`;
}

/** Format a byte count to a human-readable size string (e.g. "1.2 MB", "0 B") */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
