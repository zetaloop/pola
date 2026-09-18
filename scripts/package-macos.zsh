#!/usr/bin/env zsh
set -e

cargo build --release

app=target/release/pola.app
version=$(cargo metadata --no-deps --format-version 1 | plutil -extract packages.0.version raw -o - -)

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/pola "$app/Contents/MacOS/pola"
cp assets/pola.icns "$app/Contents/Resources/pola.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDisplayName</key>
    <string>pola</string>
    <key>CFBundleExecutable</key>
    <string>pola</string>
    <key>CFBundleIconFile</key>
    <string>pola</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.zetaloop.pola</string>
    <key>CFBundleName</key>
    <string>pola</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSAppleEventsUsageDescription</key>
    <string>pola changes the system light and dark appearance.</string>
</dict>
</plist>
EOF

codesign --force --sign - "$app"
print "$app"
