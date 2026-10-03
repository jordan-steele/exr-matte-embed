#!/usr/bin/env bash
set -euo pipefail

task_repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$task_repo_root"
if [[ "$(uname -s)" != Darwin ]]; then
    echo "Run this script on macOS." >&2
    exit 1
fi

cargo build --release --locked --bin exr-matte-embed --bin exr-matte-embed-cli
task_target_dir="${CARGO_TARGET_DIR:-$task_repo_root/target}"
task_release_dir="$task_target_dir/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}release"
task_bundle="$task_target_dir/EXR Matte Embed.app"
task_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)"
task_numeric_version="${task_version%%-*}"
mkdir -p "$task_bundle/Contents/MacOS" "$task_bundle/Contents/Resources"
mkdir -p "$task_bundle/Contents/Resources/licenses"
cp licenses/egui-phosphor-MIT.txt licenses/phosphor-icons-MIT.txt "$task_bundle/Contents/Resources/licenses/"
cp "$task_release_dir/exr-matte-embed" "$task_bundle/Contents/MacOS/EXR Matte Embed"
cp images/icon.icns "$task_bundle/Contents/Resources/icon.icns"
cat > "$task_bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
    <key>CFBundleExecutable</key><string>EXR Matte Embed</string>
    <key>CFBundleIdentifier</key><string>com.exrtools.matte-embed</string>
    <key>CFBundleName</key><string>EXR Matte Embed</string>
    <key>CFBundleDisplayName</key><string>EXR Matte Embed</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleIconFile</key><string>icon.icns</string>
    <key>CFBundleShortVersionString</key><string>$task_numeric_version</string>
    <key>CFBundleVersion</key><string>$task_numeric_version</string>
    <key>CFBundleGetInfoString</key><string>EXR Matte Embed $task_version</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
</dict></plist>
PLIST
codesign --force --deep --sign - "$task_bundle"
cp "$task_release_dir/exr-matte-embed-cli" "$task_target_dir/exr-matte-embed-cli-macos-$(uname -m)"
ditto -c -k --keepParent "$task_bundle" "$task_target_dir/EXR-Matte-Embed-macos-$(uname -m).zip"
printf 'App: %s\nCLI: %s\n' "$task_bundle" "$task_release_dir/exr-matte-embed-cli"
