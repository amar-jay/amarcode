#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dark_image="${1:-$repo_dir/assets/amarcode-dark.png}"
light_image="${2:-$repo_dir/assets/amarcode-light.png}"
output_image="${3:-$repo_dir/assets/amarcode-theme-split.png}"

if ! command -v magick >/dev/null 2>&1; then
  echo "ImageMagick is required (missing 'magick' command)." >&2
  exit 1
fi

dark_size="$(magick identify -format '%wx%h' "$dark_image")"
light_size="$(magick identify -format '%wx%h' "$light_image")"
if [[ "$dark_size" != "$light_size" ]]; then
  echo "Screenshots must have identical dimensions: dark=$dark_size light=$light_size" >&2
  exit 1
fi

width="${dark_size%x*}"
height="${dark_size#*x}"
top_x=$((width * 68 / 100))
bottom_x=$((width * 32 / 100))
temp_image="$(mktemp --suffix=.png)"
trap 'rm -f "$temp_image"' EXIT

# Start with the light screenshot, then reveal the dark screenshot to the
# upper-left of the diagonal. Both inputs remain at their original resolution.
magick \
  "$light_image" \
  "$dark_image" \
  \( -size "${width}x${height}" xc:black \
     -fill white \
     -draw "polygon 0,0 $top_x,0 $bottom_x,$height 0,$height" \) \
  -composite \
  -stroke 'rgba(184,155,101,0.72)' \
  -strokewidth 1 \
  -draw "line $top_x,0 $bottom_x,$height" \
  -strip \
  -define png:exclude-chunks=date,time \
  "$temp_image"

mv "$temp_image" "$output_image"
trap - EXIT
echo "Created $output_image (${width}x${height})"
