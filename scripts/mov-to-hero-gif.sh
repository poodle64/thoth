#!/usr/bin/env bash
# Convert a screen recording (.mov) to an optimised hero GIF for the README.
#
# Usage:
#   ./scripts/mov-to-hero-gif.sh <input.mov> [output.gif]
#
# The output defaults to docs/screenshots/hero.gif
#
# Options (via environment variables):
#   WIDTH=800        Target width in pixels (default: 800)
#   FPS=12           Frame rate (default: 12, good balance of smoothness/size)
#   TRIM_START=0     Seconds to trim from start
#   TRIM_END=0       Seconds to trim from end
#
# Requirements: ffmpeg

set -euo pipefail

INPUT="${1:?Usage: $0 <input.mov> [output.gif]}"
OUTPUT="${2:-docs/screenshots/hero.gif}"
WIDTH="${WIDTH:-800}"
FPS="${FPS:-12}"
TRIM_START="${TRIM_START:-0}"
TRIM_END="${TRIM_END:-0}"

if [[ ! -f "$INPUT" ]]; then
  echo "Error: file not found: $INPUT" >&2
  exit 1
fi

command -v ffmpeg >/dev/null 2>&1 || { echo "Error: ffmpeg required" >&2; exit 1; }

mkdir -p "$(dirname "$OUTPUT")"

# Get duration for end-trim calculation
DURATION=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$INPUT")

# Build trim filter
TRIM_FILTER=""
if (( $(echo "$TRIM_START > 0" | bc -l) )) || (( $(echo "$TRIM_END > 0" | bc -l) )); then
  END_TIME=$(echo "$DURATION - $TRIM_END" | bc -l)
  TRIM_FILTER="trim=start=${TRIM_START}:end=${END_TIME},setpts=PTS-STARTPTS,"
fi

echo "Converting: $INPUT"
echo "  Output:   $OUTPUT"
echo "  Width:    ${WIDTH}px @ ${FPS}fps"
echo "  Duration: ${DURATION}s (trim start=${TRIM_START}s end=${TRIM_END}s)"

# Two-pass approach for best quality:
# Pass 1: Generate optimised palette from the actual content
# Pass 2: Use that palette to create the GIF
PALETTE=$(mktemp /tmp/palette-XXXXXX.png)
trap 'rm -f "$PALETTE"' EXIT

FILTERS="${TRIM_FILTER}fps=${FPS},scale=${WIDTH}:-1:flags=lanczos"

ffmpeg -y -i "$INPUT" \
  -vf "${FILTERS},palettegen=stats_mode=diff" \
  "$PALETTE" 2>/dev/null

ffmpeg -y -i "$INPUT" -i "$PALETTE" \
  -lavfi "${FILTERS} [x]; [x][1:v] paletteuse=dither=bayer:bayer_scale=5" \
  "$OUTPUT" 2>/dev/null

SIZE=$(du -h "$OUTPUT" | cut -f1 | xargs)
FRAMES=$(ffprobe -v error -count_frames -select_streams v:0 \
  -show_entries stream=nb_read_frames -of csv=p=0 "$OUTPUT" 2>/dev/null || echo "?")

echo "Done: $OUTPUT ($SIZE, ${FRAMES} frames)"

# Warn if too large for GitHub
BYTES=$(stat -f%z "$OUTPUT" 2>/dev/null || stat -c%s "$OUTPUT" 2>/dev/null)
if (( BYTES > 10000000 )); then
  echo ""
  echo "Warning: GIF is over 10 MB. Consider:"
  echo "  - Reducing FPS:   FPS=8 $0 $INPUT"
  echo "  - Reducing width: WIDTH=640 $0 $INPUT"
  echo "  - Trimming:       TRIM_START=1 TRIM_END=1 $0 $INPUT"
fi
