#!/usr/bin/env python3
"""Generate all Thoth app icons from the 𓅝 ibis hieroglyph.

Requires Pillow and the Noto Sans Egyptian Hieroglyphs font (bundled with macOS).

Usage:
    nix shell --impure --expr 'let pkgs = import <nixpkgs> {}; in pkgs.python3.withPackages (ps: [ps.pillow])' \
        --command python3 scripts/generate_icons.py

    Then generate .icns separately:
        iconutil -c icns /tmp/thoth.iconset -o src-tauri/icons/icon.icns

Design:
    - App icon: Scribe's Amber (#D08B3E) glyph on Papyrus Dark (#1C1B1A)
    - Tray idle: black glyph on transparent (template icon, macOS tints)
    - Tray recording: amber glyph on transparent
    - Favicon: same as app icon, scaled to 32x32
"""

from pathlib import Path
import os
import tempfile

from PIL import Image, ImageDraw, ImageFont, ImageFilter

# Resolve paths relative to this script (project root)
SCRIPT_DIR = Path(__file__).resolve().parent
PROJECT_ROOT = SCRIPT_DIR.parent
ICONS_DIR = PROJECT_ROOT / "src-tauri" / "icons"
STATIC_DIR = PROJECT_ROOT / "static"

FONT_PATH = "/System/Library/Fonts/Supplemental/NotoSansEgyptianHieroglyphs-Regular.ttf"
CHAR = chr(0x1315D)

# Branding colours
AMBER = (208, 139, 62, 255)
PAPYRUS_DARK = (28, 27, 26, 255)
BORDER_COLOUR = (54, 51, 48, 255)

# Glyph scale: 66% of icon size (adjust to make bird larger/smaller)
GLYPH_SCALE = 0.66


# ============================================================================
# App Icons (amber glyph on dark background with rounded corners)
# ============================================================================

def create_app_icon(size: int, corner_radius_ratio: float = 0.1875) -> Image.Image:
    """Create app icon: amber glyph on dark bg with rounded corners."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # Rounded rectangle background
    radius = int(size * corner_radius_ratio)
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=PAPYRUS_DARK)

    # Subtle inner border
    inset = max(1, size // 32)
    inner_radius = max(1, radius - inset)
    draw.rounded_rectangle(
        [inset, inset, size - 1 - inset, size - 1 - inset],
        radius=inner_radius,
        fill=None,
        outline=BORDER_COLOUR,
        width=max(1, size // 512),
    )

    # Render the hieroglyph
    font_size = int(size * GLYPH_SCALE)
    font = ImageFont.truetype(FONT_PATH, font_size)

    # Get bounding box to centre properly
    bbox = font.getbbox(CHAR)
    glyph_w = bbox[2] - bbox[0]
    glyph_h = bbox[3] - bbox[1]

    # Centre the glyph (slight upward shift looks better)
    x = (size - glyph_w) // 2 - bbox[0]
    y = (size - glyph_h) // 2 - bbox[1] - int(size * 0.02)

    draw.text((x, y), CHAR, font=font, fill=AMBER)

    return img


# ============================================================================
# Tray Icons (glyph silhouette on transparent background)
# ============================================================================

def create_tray_icon(size: int, r: int, g: int, b: int) -> Image.Image:
    """Create a tray icon: filled glyph silhouette on transparent background.

    The Noto font renders 𓅝 as an outline/line drawing. For a proper menu bar
    icon we need a solid filled shape. Strategy:
    1. Render the glyph in white on a black background at high resolution.
    2. Threshold to create a binary mask of the glyph strokes.
    3. Flood-fill the exterior (black region reachable from edges) with grey.
    4. Everything that's still black is an interior enclosed region — fill it white.
    5. Combine stroke mask + interior fill → solid silhouette alpha mask.
    6. Apply the desired colour using the mask as alpha.
    """
    # Render at 8x for quality
    render_size = size * 8
    font_size = int(render_size * 0.85)
    font = ImageFont.truetype(FONT_PATH, font_size)

    # Step 1: Render glyph in white on black
    canvas = Image.new("L", (render_size, render_size), 0)
    draw = ImageDraw.Draw(canvas)

    bbox = font.getbbox(CHAR)
    glyph_w = bbox[2] - bbox[0]
    glyph_h = bbox[3] - bbox[1]

    x = (render_size - glyph_w) // 2 - bbox[0]
    y = (render_size - glyph_h) // 2 - bbox[1]

    draw.text((x, y), CHAR, font=font, fill=255)

    # Step 2: Threshold to get a clean stroke mask
    stroke_mask = canvas.point(lambda p: 255 if p > 30 else 0)

    # Step 3: Flood-fill exterior from all edges with a marker value (128)
    # Use a copy to identify which black pixels are exterior
    fill_canvas = stroke_mask.copy()
    from PIL import ImageDraw as ID2
    fill_draw = ID2.Draw(fill_canvas)

    # Flood fill from all four edges to mark exterior
    from collections import deque
    pixels = fill_canvas.load()
    w, h = fill_canvas.size
    visited = set()
    queue = deque()

    # Seed from all edge pixels that are black (exterior)
    for i in range(w):
        if pixels[i, 0] == 0:
            queue.append((i, 0))
        if pixels[i, h - 1] == 0:
            queue.append((i, h - 1))
    for j in range(h):
        if pixels[0, j] == 0:
            queue.append((0, j))
        if pixels[w - 1, j] == 0:
            queue.append((w - 1, j))

    # BFS flood fill exterior
    while queue:
        cx, cy = queue.popleft()
        if (cx, cy) in visited:
            continue
        if cx < 0 or cx >= w or cy < 0 or cy >= h:
            continue
        if pixels[cx, cy] != 0:  # Stop at glyph strokes
            continue
        visited.add((cx, cy))
        pixels[cx, cy] = 128  # Mark as exterior
        queue.append((cx + 1, cy))
        queue.append((cx - 1, cy))
        queue.append((cx, cy + 1))
        queue.append((cx, cy - 1))

    # Step 4: Anything still black (value 0) is interior — set to white
    # Combine: stroke (255) + interior (0→255) + exterior (128→0)
    alpha_mask = fill_canvas.point(lambda p: 0 if p == 128 else 255)

    # Step 5: Create the final RGBA image
    img = Image.new("RGBA", (render_size, render_size), (r, g, b, 255))
    img.putalpha(alpha_mask)

    return img.resize((size, size), Image.LANCZOS)


# ============================================================================
# Main
# ============================================================================

def main() -> None:
    # --- App icons (Tauri-required sizes) ---
    app_icon_sizes = {
        "icon.png": 1024,
        "32x32.png": 32,
        "128x128.png": 128,
        "128x128@2x.png": 256,
    }

    for filename, size in app_icon_sizes.items():
        if size < 64:
            # Small sizes: render at 4x and downscale for quality
            big = create_app_icon(size * 4)
            img = big.resize((size, size), Image.LANCZOS)
        else:
            img = create_app_icon(size)

        path = ICONS_DIR / filename
        img.save(path)
        print(f"  {filename} ({size}x{size}): {os.path.getsize(path)} bytes")

    # --- Favicon ---
    favicon = create_app_icon(128)
    favicon_32 = favicon.resize((32, 32), Image.LANCZOS)
    favicon_path = STATIC_DIR / "favicon.png"
    favicon_32.save(favicon_path)
    print(f"  favicon.png: {os.path.getsize(favicon_path)} bytes")

    # --- Tray icons (22 standard, 44 Retina) ---
    for tray_size in [22, 44]:
        idle = create_tray_icon(tray_size, 0, 0, 0)
        idle_path = ICONS_DIR / f"tray-idle-{tray_size}.png"
        idle.save(idle_path)
        print(f"  tray-idle-{tray_size}.png: {os.path.getsize(idle_path)} bytes")

        rec = create_tray_icon(tray_size, *AMBER[:3])
        rec_path = ICONS_DIR / f"tray-recording-{tray_size}.png"
        rec.save(rec_path)
        print(f"  tray-recording-{tray_size}.png: {os.path.getsize(rec_path)} bytes")

    # --- .icns (macOS iconset → iconutil) ---
    iconset_dir = Path(tempfile.mkdtemp()) / "thoth.iconset"
    iconset_dir.mkdir()

    iconset_sizes = {
        "icon_16x16.png": 16,
        "icon_16x16@2x.png": 32,
        "icon_32x32.png": 32,
        "icon_32x32@2x.png": 64,
        "icon_128x128.png": 128,
        "icon_128x128@2x.png": 256,
        "icon_256x256.png": 256,
        "icon_256x256@2x.png": 512,
        "icon_512x512.png": 512,
        "icon_512x512@2x.png": 1024,
    }

    for filename, size in iconset_sizes.items():
        if size < 64:
            big = create_app_icon(size * 4)
            img = big.resize((size, size), Image.LANCZOS)
        else:
            img = create_app_icon(size)
        img.save(iconset_dir / filename)

    icns_path = ICONS_DIR / "icon.icns"
    ret = os.system(f"iconutil -c icns '{iconset_dir}' -o '{icns_path}'")
    if ret == 0:
        print(f"  icon.icns: {os.path.getsize(icns_path)} bytes")
    else:
        print("  WARNING: iconutil failed (macOS only)")

    # --- .ico (Windows, multi-size) ---
    src_256 = create_app_icon(256)
    ico_path = ICONS_DIR / "icon.ico"
    src_256.save(
        ico_path,
        format="ICO",
        sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print(f"  icon.ico: {os.path.getsize(ico_path)} bytes")

    print("\nAll icons generated successfully!")


if __name__ == "__main__":
    main()
