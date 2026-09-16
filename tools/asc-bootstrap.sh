#!/usr/bin/env bash
# Create everything Apple needs for signing, through the App Store
# Connect API: the two distribution certificates, the registered bundle
# id, and the Mac App Store provisioning profile.
#
# Apple never sees a private key here. Each certificate starts as a key
# generated on this Mac; only the signing request crosses the wire, and
# the certificate that comes back is the public half.
#
#   ASC_KEY_ID=… ASC_ISSUER_ID=… ./tools/asc-bootstrap.sh
#
# Safe to re-run: it skips anything that already exists.
set -euo pipefail
cd "$(dirname "$0")/.."

BUNDLE=com.craftedup.lonetypist
APP_NAME="Lone Typist"
OUT=target/signing
ASC=./tools/asc.rb

: "${ASC_KEY_ID:?set ASC_KEY_ID (the 10-character key id)}"
: "${ASC_ISSUER_ID:?set ASC_ISSUER_ID (the issuer UUID from App Store Connect -> Users and Access -> Integrations)}"

mkdir -p "$OUT"
chmod 700 "$OUT"

# --- certificates ----------------------------------------------------------
# MAC_APP_DISTRIBUTION signs the app, MAC_INSTALLER_DISTRIBUTION signs the
# installer package. They are separate certificates and both are required.

make_cert() {
    local type=$1 keychain_name=$2 key="$OUT/$1.key" csr="$OUT/$1.csr"
    local cer="$OUT/$1.cer" pem="$OUT/$1.pem" p12="$OUT/$1.p12"

    if security find-identity -v | grep -q "$keychain_name"; then
        echo "==> $keychain_name already in the keychain, skipping"
        return
    fi

    echo "==> requesting $type"
    openssl req -new -newkey rsa:2048 -nodes -keyout "$key" -out "$csr" \
        -subj "/emailAddress=sean@craftedup.com/CN=$APP_NAME/C=US" 2>/dev/null
    chmod 600 "$key"

    ruby -rjson -e 'puts JSON.dump(data: {type: "certificates", attributes: {
        certificateType: ARGV[0], csrContent: File.read(ARGV[1])}})' \
        "$type" "$csr" > "$OUT/$type.body.json"

    $ASC post /v1/certificates "$(cat "$OUT/$type.body.json")" > "$OUT/$type.response.json"

    ruby -rjson -rbase64 -e '
        content = JSON.parse(File.read(ARGV[0])).dig("data", "attributes", "certificateContent")
        File.binwrite(ARGV[1], Base64.decode64(content))' \
        "$OUT/$type.response.json" "$cer"

    openssl x509 -inform DER -in "$cer" -out "$pem"
    openssl pkcs12 -export -out "$p12" -inkey "$key" -in "$pem" -passout pass: 2>/dev/null
    chmod 600 "$p12"

    # Importing may ask for the login keychain password the first time.
    security import "$p12" -P "" -T /usr/bin/codesign -T /usr/bin/productbuild
    echo "    imported $keychain_name"
}

make_cert MAC_APP_DISTRIBUTION "Apple Distribution"
make_cert MAC_INSTALLER_DISTRIBUTION "Mac Installer Distribution"

# --- bundle id -------------------------------------------------------------

echo "==> checking the bundle id"
BUNDLE_RESOURCE=$($ASC get /v1/bundleIds "filter\[identifier\]=$BUNDLE" \
    | ruby -rjson -e 'puts (JSON.parse($stdin.read)["data"].first || {})["id"]')

if [ -z "$BUNDLE_RESOURCE" ]; then
    echo "    registering $BUNDLE"
    BUNDLE_RESOURCE=$($ASC post /v1/bundleIds "$(ruby -rjson -e 'puts JSON.dump(
        data: {type: "bundleIds", attributes: {
            identifier: ARGV[0], name: ARGV[1], platform: "MAC_OS"}})' \
        "$BUNDLE" "$APP_NAME")" | ruby -rjson -e 'puts JSON.parse($stdin.read).dig("data", "id")')
else
    echo "    already registered ($BUNDLE_RESOURCE)"
fi

# --- provisioning profile --------------------------------------------------

PROFILE="$OUT/lonetypist.provisionprofile"
echo "==> provisioning profile"

CERT_ID=$($ASC get /v1/certificates "limit=200" | ruby -rjson -e '
    certs = JSON.parse($stdin.read)["data"]
    match = certs.find { |c| c.dig("attributes", "certificateType") == "MAC_APP_DISTRIBUTION" }
    abort "no MAC_APP_DISTRIBUTION certificate" unless match
    puts match["id"]')

# A profile is pinned to the certificates it was made with, so a new
# certificate means a new profile.
EXISTING=$($ASC get /v1/profiles "limit=200" | ruby -rjson -e '
    profiles = JSON.parse($stdin.read)["data"]
    match = profiles.find { |p|
        p.dig("attributes", "name") == ARGV[0] &&
        p.dig("attributes", "profileState") == "ACTIVE" }
    puts match ? match["id"] : ""' "$APP_NAME Mac App Store")

if [ -n "$EXISTING" ]; then
    echo "    reusing the active profile ($EXISTING)"
    $ASC get "/v1/profiles/$EXISTING" | ruby -rjson -rbase64 -e '
        content = JSON.parse($stdin.read).dig("data", "attributes", "profileContent")
        File.binwrite(ARGV[0], Base64.decode64(content))' "$PROFILE"
else
    echo "    creating one"
    BODY=$(ruby -rjson -e 'puts JSON.dump(data: {
        type: "profiles",
        attributes: {name: ARGV[0], profileType: "MAC_APP_STORE"},
        relationships: {
            bundleId: {data: {type: "bundleIds", id: ARGV[1]}},
            certificates: {data: [{type: "certificates", id: ARGV[2]}]}}})' \
        "$APP_NAME Mac App Store" "$BUNDLE_RESOURCE" "$CERT_ID")
    $ASC post /v1/profiles "$BODY" | ruby -rjson -rbase64 -e '
        content = JSON.parse($stdin.read).dig("data", "attributes", "profileContent")
        File.binwrite(ARGV[0], Base64.decode64(content))' "$PROFILE"
fi

echo
echo "Ready. The profile is at $PROFILE"
echo "Build and validate with:"
echo "  ASC_KEY_ID=$ASC_KEY_ID ASC_ISSUER_ID=\$ASC_ISSUER_ID PROFILE=$PROFILE ./tools/appstore.sh"
