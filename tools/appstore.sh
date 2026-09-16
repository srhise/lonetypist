#!/usr/bin/env bash
# Build, sign and upload the Mac App Store build.
#
# The App Store build is the same binary as the direct download, signed
# differently: sandboxed, with an embedded provisioning profile, wrapped
# in a signed installer package. It detects the sandbox at runtime and
# asks for a writing folder on first launch.
#
# One-time setup, all under the Crafted team:
#   1. App Store Connect -> Users and Access -> Integrations: create an
#      API key, download AuthKey_<KEYID>.p8 to
#      ~/.appstoreconnect/private_keys/, and note the Key ID and Issuer ID.
#   2. Certificates: an "Apple Distribution" and a "3rd Party Mac
#      Developer Installer" certificate in the keychain.
#   3. Identifiers: register com.craftedup.lonetypist.
#   4. Profiles: a "Mac App Store" provisioning profile for that ID,
#      saved somewhere and pointed at by PROFILE below.
#
# Usage:
#   ASC_KEY_ID=XXXX ASC_ISSUER_ID=yyyy-… PROFILE=~/lonetypist.provisionprofile \
#       ./tools/appstore.sh [--upload]
#
# Without --upload it stops after validation, which is the safe default:
# an upload consumes the build number and cannot be taken back.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

UPLOAD=no
[ "${1:-}" = "--upload" ] && UPLOAD=yes

VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
BUILD=${BUILD:-$(date +%Y%m%d%H%M)}   # must increase with every upload
APP="target/appstore/Lone Typist.app"
PKG="target/appstore/LoneTypist-$VERSION.pkg"

fail() { echo "error: $*" >&2; exit 1; }

# --- preflight -------------------------------------------------------------

# The same certificate goes by two names: Xcode calls it "Apple
# Distribution", while a MAC_APP_DISTRIBUTION one issued through the API
# arrives as "3rd Party Mac Developer Application". Either signs a Mac
# App Store build.
APP_CERT=$(security find-identity -v -p codesigning \
    | awk -F'"' '/Apple Distribution|3rd Party Mac Developer Application/ {print $2; exit}')
[ -n "$APP_CERT" ] || fail 'no Mac App Store application certificate in the
keychain. Run ./tools/asc-bootstrap.sh to create one.'

PKG_CERT=$(security find-identity -v \
    | awk -F'"' '/3rd Party Mac Developer Installer|Mac Installer Distribution/ {print $2; exit}')
[ -n "$PKG_CERT" ] || fail 'no "Mac Installer Distribution" certificate in the
keychain. It is a separate certificate from the application one, and the
package cannot be signed without it. Run ./tools/asc-bootstrap.sh.'

PROFILE=${PROFILE:-}
[ -n "$PROFILE" ] && [ -f "$PROFILE" ] \
    || fail "set PROFILE to a Mac App Store provisioning profile for
com.craftedup.lonetypist (developer.apple.com -> Profiles)."

: "${ASC_KEY_ID:?set ASC_KEY_ID to the App Store Connect API key id}"
: "${ASC_ISSUER_ID:?set ASC_ISSUER_ID to the App Store Connect issuer id}"

# --- build -----------------------------------------------------------------

echo "==> tests"
cargo test --release --quiet

echo "==> building universal binary"
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
mkdir -p target/universal target/appstore
lipo -create \
    target/aarch64-apple-darwin/release/lonetypist \
    target/x86_64-apple-darwin/release/lonetypist \
    -output target/universal/lonetypist

# --- icon, as an asset catalog ---------------------------------------------
# App Store Connect wants the icon compiled into Assets.car, not only the
# .icns that the direct download ships.

echo "==> compiling the icon catalog"
python3 tools/make-icon.py target/icon.bmp
ICONSET=target/lonetypist.iconset
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
sips -s format png target/icon.bmp --out target/icon.png >/dev/null
for size in 16 32 64 128 256 512 1024; do
    sips -z $size $size target/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
done
for size in 16 32 128 256 512; do
    cp "$ICONSET/icon_$((size*2))x$((size*2)).png" "$ICONSET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$ICONSET" -o target/lonetypist.icns

CATALOG=target/appstore/Assets.xcassets
rm -rf "$CATALOG"; mkdir -p "$CATALOG/AppIcon.appiconset"
printf '{\n  "info" : { "version" : 1, "author" : "xcode" }\n}\n' > "$CATALOG/Contents.json"
python3 - "$CATALOG/AppIcon.appiconset" "$ICONSET" <<'PY'
import json, shutil, sys
from pathlib import Path
out, iconset = Path(sys.argv[1]), Path(sys.argv[2])
images = []
for size in (16, 32, 128, 256, 512):
    for scale in (1, 2):
        src = iconset / (f"icon_{size}x{size}.png" if scale == 1 else f"icon_{size}x{size}@2x.png")
        name = f"icon_{size}{'' if scale == 1 else '@2x'}.png"
        shutil.copy(src, out / name)
        images.append({"idiom": "mac", "size": f"{size}x{size}",
                       "scale": f"{scale}x", "filename": name})
json.dump({"images": images, "info": {"version": 1, "author": "xcode"}},
          open(out / "Contents.json", "w"), indent=2)
PY

# --- assemble the bundle ---------------------------------------------------

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/universal/lonetypist "$APP/Contents/MacOS/lonetypist"
cp target/lonetypist.icns "$APP/Contents/Resources/lonetypist.icns"
cp "$PROFILE" "$APP/Contents/embedded.provisionprofile"

xcrun actool "$CATALOG" --compile "$APP/Contents/Resources" --app-icon AppIcon \
    --minimum-deployment-target 11.0 --platform macosx \
    --output-partial-info-plist target/appstore/icon.plist --errors --warnings \
    > target/appstore/actool.log 2>&1 \
    || { cat target/appstore/actool.log; fail "actool failed"; }

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>              <string>Lone Typist</string>
    <key>CFBundleDisplayName</key>       <string>Lone Typist</string>
    <key>CFBundleIdentifier</key>        <string>com.craftedup.lonetypist</string>
    <key>CFBundleVersion</key>           <string>$BUILD</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleExecutable</key>        <string>lonetypist</string>
    <key>CFBundleIconFile</key>          <string>lonetypist</string>
    <key>CFBundleIconName</key>          <string>AppIcon</string>
    <key>CFBundlePackageType</key>       <string>APPL</string>
    <key>LSMinimumSystemVersion</key>    <string>11.0</string>
    <key>LSApplicationCategoryType</key> <string>public.app-category.productivity</string>
    <key>NSHighResolutionCapable</key>   <true/>
    <key>ITSAppUsesNonExemptEncryption</key> <false/>
    <key>NSHumanReadableCopyright</key>  <string>MIT licensed. Source at github.com/srhise/lonetypist</string>
</dict>
</plist>
PLIST

# Uploads are rejected if anything inside the app is quarantined.
xattr -cr "$APP"

# The signature has to carry the same application identifier the embedded
# profile does, or the build validates but cannot be used with TestFlight
# (warning 90886). Both values are read back out of the profile rather
# than written down a second time and left to drift.
echo "==> preparing entitlements"
security cms -D -i "$PROFILE" > target/appstore/profile.plist
# plutil reads "." as a key-path separator, so the dots in these key
# names have to reach it escaped -- which means single quotes, since bash
# would otherwise eat the backslashes on the way past.
APP_ID=$(plutil -extract 'Entitlements.com\.apple\.application-identifier' raw -o - target/appstore/profile.plist)
TEAM_ID=$(plutil -extract 'Entitlements.com\.apple\.developer\.team-identifier' raw -o - target/appstore/profile.plist)
[ -n "$APP_ID" ] && [ -n "$TEAM_ID" ] || fail "could not read the identifiers out of $PROFILE"

SIGNING_ENTITLEMENTS=target/appstore/signing.entitlements
cp tools/appstore.entitlements "$SIGNING_ENTITLEMENTS"
plutil -insert 'com\.apple\.application-identifier' -string "$APP_ID" "$SIGNING_ENTITLEMENTS"
plutil -insert 'com\.apple\.developer\.team-identifier' -string "$TEAM_ID" "$SIGNING_ENTITLEMENTS"
echo "    $APP_ID"

echo "==> signing the app ($APP_CERT)"
codesign --force --timestamp --options runtime \
    --entitlements "$SIGNING_ENTITLEMENTS" \
    --sign "$APP_CERT" "$APP"
codesign --verify --strict --verbose=2 "$APP"

echo "==> checking the sandbox actually took"
codesign -d --entitlements - --xml "$APP" 2>/dev/null \
    | plutil -convert xml1 -o - - \
    | grep -q "com.apple.security.app-sandbox" \
    || fail "the signed app has no sandbox entitlement"

echo "==> building $PKG"
rm -f "$PKG"
productbuild --component "$APP" /Applications --sign "$PKG_CERT" "$PKG"

# --- validate, and only then upload ----------------------------------------

echo "==> validating with App Store Connect"
xcrun altool --validate-app -f "$PKG" -t macos \
    --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"

if [ "$UPLOAD" = yes ]; then
    echo "==> uploading build $BUILD"
    xcrun altool --upload-app -f "$PKG" -t macos \
        --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
    echo
    echo "Uploaded. It appears in App Store Connect under TestFlight/Builds"
    echo "after processing; attach it to the version and submit for review."
else
    echo
    echo "Validated but not uploaded. Re-run with --upload to send build $BUILD."
fi
