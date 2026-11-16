#!/bin/bash
# Build the macOS download by hand, on a Mac that holds the signing identity:
# a universal app, signed with a Developer ID from the keychain, its DMG
# notarized through a notarytool keychain profile and stapled.
#
#   APPLE_SIGNING_IDENTITY="Developer ID Application: …" \
#   PURICAT_NOTARY_PROFILE=<profile> scripts/release-mac.sh
#
# The profile is one made with `xcrun notarytool store-credentials`. Without
# PURICAT_NOTARY_PROFILE the app is signed but not notarized; without
# APPLE_SIGNING_IDENTITY it is only ad-hoc signed. No secret is read or
# written by this script: codesign and notarytool take them from the keychain.
#
# The result is target/release-download/PuriCat-mac.dmg with SHA256SUMS.txt,
# under the names the README links to. Attach them to a release with
#   gh release upload <tag> target/release-download/* --clobber
# The release workflow on GitHub does the same from the repository's secrets.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:--}"

bundle=target/universal-apple-darwin/release/bundle
rm -f "$bundle"/dmg/*.dmg
pnpm tauri build --target universal-apple-darwin
app="$bundle/macos/PuriCat.app"
dmg=$(ls "$bundle"/dmg/*.dmg)
codesign --verify --deep --strict --verbose=2 "$app"
lipo -archs "$app/Contents/MacOS/puricat"

if [ "$APPLE_SIGNING_IDENTITY" != "-" ]; then
  # Tauri signed the app; the DMG around it gets its own signature.
  codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
  if [ -n "${PURICAT_NOTARY_PROFILE:-}" ]; then
    xcrun notarytool submit "$dmg" --keychain-profile "$PURICAT_NOTARY_PROFILE" --wait
    xcrun stapler staple "$dmg"
    xcrun stapler validate "$dmg"
    spctl -a -vv -t install "$dmg"
  else
    echo "! PURICAT_NOTARY_PROFILE is not set: signed, not notarized"
  fi
else
  echo "! APPLE_SIGNING_IDENTITY is not set: ad-hoc signed only"
fi

out=target/release-download
mkdir -p "$out"
cp "$dmg" "$out/PuriCat-mac.dmg"
(cd "$out" && shasum -a 256 PuriCat-mac.dmg > SHA256SUMS.txt && cat SHA256SUMS.txt)
ls -la "$out"
