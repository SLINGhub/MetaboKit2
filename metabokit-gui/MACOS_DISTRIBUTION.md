# Distributing MetaboKit on macOS

MetaboKit (DDA and DIA) is distributed outside the Mac App Store as a DMG. A PKG
is not needed: users open the DMG and drag **MetaboKit** to Applications.
Version 0.3 uses `MetaboKit GUI.app`; an older `MetaboKit DDA.app` can remain alongside
it until the user removes that older copy. The bundle identifier is unchanged.

The public release command produces one universal app for Apple Silicon and
Intel Macs. It signs the app with a Developer ID Application certificate,
submits the signed DMG to Apple for notarization, staples the approval ticket,
and checks the result with Gatekeeper.

## One-time setup

### 1. Developer ID certificate

Install a valid **Developer ID Application** certificate, including its private
key, in the login keychain. Confirm it is available:

```sh
security find-identity -v -p codesigning
```

The output must contain `Developer ID Application` and report at least one
valid identity.

### 2. Notarization credential

Create an app-specific password at <https://account.apple.com>, then save it in
the macOS Keychain through Apple's tool. Replace the email address below with
the Apple Account used for the developer team:

```sh
xcrun notarytool store-credentials "metabokit-notary" \
  --apple-id "YOUR_APPLE_ACCOUNT_EMAIL" \
  --team-id "L6FU3FBMXX"
```

The tool securely prompts for the app-specific password and validates it before
saving. Do not put that password in this repository or directly on the command
line.

An App Store Connect API key can be used instead. See `xcrun notarytool
store-credentials --help` for the `--key`, `--key-id`, and `--issuer` options.

## Build a public release

From the `metabokit-gui` directory:

```sh
./build-macos.sh --release --notary-profile "metabokit-notary" --universal
```

Tests and notarization are part of this command. The first notarization can take
longer than subsequent releases. The final DMG is under:

```text
target/universal-apple-darwin/release/bundle/dmg/
```

Only distribute the DMG after the script prints that it is signed, notarized,
stapled, and ready.

## Local test builds

Build an unsigned DMG for use on the current Mac:

```sh
./build-macos.sh
```

Build and verify a signed universal DMG without sending it to Apple:

```sh
./build-macos.sh --sign-only --universal
```

The sign-only result is not a public release because it has no notarization
ticket.
