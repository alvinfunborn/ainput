#!/bin/bash
# Export the shared artwork with Apple's macOS icon silhouette, like Screen Buoy.
# Generated PNG/ICNS files are checked in; only regeneration requires Xcode.
set -euo pipefail
project_dir="$(cd "$(dirname "$0")/.." && pwd)"
developer_dir="${DEVELOPER_DIR:-$(xcode-select -p)}"
ictool="$developer_dir/../Applications/Icon Composer.app/Contents/Executables/ictool"
if [[ ! -x "$ictool" ]]; then
  echo "Icon Composer is required to regenerate the macOS icons." >&2
  exit 1
fi
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
mkdir -p "$work_dir/AInput.icon/Assets" "$work_dir/ainput.iconset"
sips -z 1024 1024 "$project_dir/src-tauri/icons/icon.png" \
  --out "$work_dir/AInput.icon/Assets/icon.png" >/dev/null
cat > "$work_dir/AInput.icon/icon.json" <<'EOF'
{
  "fill": {"solid": "srgb:0.36,0.26,0.99,1.0"},
  "groups": [{"layers": [{"image-name": "icon.png", "name": "AInput"}]}],
  "supported-platforms": {"squares": "shared"}
}
EOF
case "$("$ictool" --help 2>&1)" in
  *--export-image*)
    "$ictool" "$work_dir/AInput.icon" --export-image \
      --output-file "$work_dir/artwork.png" --platform macOS \
      --rendition Default --width 824 --height 824 --scale 1
    ;;
  *)
    "$ictool" "$work_dir/AInput.icon" --export-preview \
      macOS Default 824 824 1 -45 "$work_dir/artwork.png"
    ;;
esac
sips --padToHeightWidth 1024 1024 "$work_dir/artwork.png" \
  --out "$project_dir/src-tauri/icons/icon-macos.png" >/dev/null
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$project_dir/src-tauri/icons/icon-macos.png" \
    --out "$work_dir/ainput.iconset/icon_${size}x${size}.png" >/dev/null
  pixels=$((size * 2))
  sips -z "$pixels" "$pixels" "$project_dir/src-tauri/icons/icon-macos.png" \
    --out "$work_dir/ainput.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$work_dir/ainput.iconset" -o "$project_dir/src-tauri/icons/icon.icns"
# UI/menu-bar assets use the same rounded artwork without the app-icon margin.
for size in 32 128 256; do
  output="${size}x${size}.png"
  if [[ "$size" == 256 ]]; then output="128x128@2x.png"; fi
  sips -z "$size" "$size" "$work_dir/artwork.png" \
    --out "$project_dir/src-tauri/icons/$output" >/dev/null
done
sips -z 44 44 "$work_dir/artwork.png" \
  --out "$project_dir/src-tauri/icons/tray-macos.png" >/dev/null
echo "Generated rounded app, settings and menu-bar icons."
