# Releasing Roofle

How to build, sign, notarize and ship a Roofle release. Read this top to bottom
the first time; after that, jump to [Release checklist](#release-checklist).

**Status of the setup** (so you know what exists today):

| Piece | State |
|---|---|
| Build the DMG (`npm run tauri build`) | Works |
| Version in one place per file (all `1.0.0-beta`) | Done |
| Homebrew cask generator (`scripts/make-cask.sh`) | Done |
| Apple signing and notarization | **Needs your Developer ID** (see below) |
| `src-tauri/entitlements.plist` | Done, referenced from `tauri.conf.json` (see [Entitlements](#entitlements)) |
| Homebrew tap repo (`roofle-ai/homebrew-roofle`, must be public) | Create it once (step 6 below); update the cask after each release |
| Release CI workflow (`.github/workflows/release.yml`) | Done: builds, signs, notarizes and drafts a release; needs the Apple secrets below |

---

## 1. Where to keep the keys

Nothing secret ever goes in this repo. Not in `tauri.conf.json`, the README,
scripts, or commit history. `.gitignore` blocks `*.p12`, `*.cer`, `*.p8` and
`.env*` as a safety net.

| Secret | What it is | Where it lives |
|---|---|---|
| **Developer ID Application certificate + private key** | Signs the app | Your **login keychain** on the build Mac. Keep an encrypted **backup `.p12`** in a password manager (e.g. 1Password), protected by its own password. Losing the private key means revoking and re-issuing the certificate. |
| **`.p12` password** | Protects the backup | Password manager |
| **App-specific password** | Lets `notarytool` log in as your Apple ID | Password manager. Create at appleid.apple.com → Sign-In and Security → App-Specific Passwords. Never use your real Apple ID password. |
| **Team ID**, **signing identity name** | Identify who signed | Not secret. Safe in docs and config. |
| **App Store Connect API key (`.p8`)** (optional, better for CI) | Alternative to the Apple ID login for notarization | Password manager, plus CI secret storage |
| **Tauri updater signing key** (`npx tauri signer generate`; public key goes in `tauri.conf.json`) | Signs update bundles | Password manager and CI secrets. Losing it means existing installs can never update. |

**On your Mac:** keep secrets in the keychain or a password manager and pass them
to the build as environment variables for that one command. Don't `export` them
in a dotfile that gets synced anywhere.

**In CI (GitHub Actions):** store each as a secret in a GitHub **Environment**
named `release` (Settings → Environments; add required reviewers), never in the
workflow file:
`APPLE_CERTIFICATE` (base64 of the `.p12`), `APPLE_CERTIFICATE_PASSWORD`,
`APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`,
`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and
optionally `VITE_FEEDBACK_URL`.

**Shipping from CI:** bump the version (checklist below), merge to `main`, then
Actions → Release → Run workflow. It builds, signs and notarizes, and creates a
**draft** release `v<version>` with the DMG, the updater bundle and signature,
`latest.json` and `SHA256SUMS`. Install the DMG on a clean Mac and test the
update from the previous version, then publish the draft. Publishing is what
makes `latest.json` live for the in-app updater
(`releases/latest/download/latest.json` ignores drafts and pre-releases).

## 2. One-time setup

1. **Join the Apple Developer Program** (paid). Without it you can't notarize,
   and other Macs will show Gatekeeper warnings.
2. **Create the certificate.** In Xcode → Settings → Accounts → Manage
   Certificates → `+` → **Developer ID Application**. (Or create it at
   developer.apple.com and import it.) It lands in your login keychain.
3. **Find its exact name:**
   ```sh
   security find-identity -v -p codesigning
   # 1) ABCD... "Developer ID Application: Your Name (TEAMID)"
   ```
   That quoted string is your `APPLE_SIGNING_IDENTITY`.
4. **Back up the certificate.** Keychain Access → My Certificates → right-click
   the certificate → Export → `.p12` with a strong password → store in your
   password manager.
5. **Create an app-specific password** for notarization (see table above).
6. **Create the tap repo.** On GitHub create `roofle-ai/homebrew-roofle` as a
   **public** repo (the `homebrew-` prefix is required; a private repo fails
   with "could not read Username"), with a README and a `Casks/` folder.

### Entitlements

The signed app uses the hardened runtime, which blocks microphone access unless
declared. `src-tauri/entitlements.plist` declares it:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.device.audio-input</key>
  <true/>
</dict>
</plist>
```

and `src-tauri/tauri.conf.json` references it:

```json
"macOS": { "infoPlist": "Info.plist", "entitlements": "entitlements.plist" }
```

System-audio (ScreenCaptureKit) capture is controlled by the user's *Screen &
System Audio Recording* permission, not an entitlement.

## 3. Release checklist

Run these on the Mac that holds the certificate.

1. **Bump the version** everywhere it appears (keep them equal):
   - `src-tauri/tauri.conf.json` (`version`; this names the DMG and app)
   - `package.json` and `package-lock.json`
   - `Cargo.toml` (workspace) and `src-tauri/Cargo.toml`, then `cargo check` to refresh `Cargo.lock`
2. **Verify:** `cargo test --workspace` and `npx tsc --noEmit`.
3. **Models are not bundled.** The app downloads them on first run, so nothing to fetch for a release build. (`scripts/fetch-assets.sh` is only for the CLI and tests.)
4. **Build, sign and notarize** with the credentials in the environment for this
   one command:
   ```sh
   export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
   export APPLE_ID="you@example.com"
   export APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx"   # app-specific password
   export APPLE_TEAM_ID="TEAMID"
   npm run tauri build
   ```
   Output: `target/release/bundle/dmg/Roofle_<version>_aarch64.dmg`
   (and `target/release/bundle/macos/Roofle.app`).
5. **Check the result** before shipping:
   ```sh
   codesign --verify --deep --strict --verbose=2 target/release/bundle/macos/Roofle.app
   spctl --assess --type execute --verbose target/release/bundle/macos/Roofle.app   # "accepted, source=Notarized Developer ID"
   codesign -dvv target/release/bundle/macos/Roofle.app 2>&1 | grep -E "Identifier|Info.plist"
   # Identifier must be com.roofle.app and Info.plist must be "entries=..." (not "not bound")
   ```
   Then open the DMG on a different Mac (or a fresh user account), drag the app
   out, and launch it. There should be no Gatekeeper warning. Grant microphone
   and Screen & System Audio Recording and confirm recording works.
6. **Tag and push:** `git tag v<version> && git push origin v<version>`
7. **Publish the build** (see [Shipping the builds](#4-shipping-the-builds)).

## 4. Shipping the builds

Two channels, both serve the same DMG:

**A. Direct download.** The Release workflow (see section 1) creates a draft
GitHub Release `v<version>` with the DMG, updater files, `latest.json` and
`SHA256SUMS`. Check it, edit the notes, and publish it (not as a pre-release).

**B. Homebrew.**
```sh
# 1. Download the DMG from the PUBLISHED release (not your local build):
curl -fL -o /tmp/Roofle.dmg \
  https://github.com/roofle-ai/roofle/releases/download/v<version>/Roofle_<version>_aarch64.dmg
# 2. Generate the cask from that file:
ROOFLE_URL_PREFIX=https://github.com/roofle-ai/roofle/releases/download \
ROOFLE_HOMEPAGE=https://github.com/roofle-ai/roofle \
  scripts/make-cask.sh /tmp/Roofle.dmg
```
This reads the version, hashes the DMG and writes
`packaging/homebrew/Casks/roofle.rb`. Copy it to `Casks/roofle.rb` in the
`roofle-ai/homebrew-roofle` repo, commit and push. Publish the release **first**
and hash the downloaded file: the cask's checksum must match the file users
download, and a CI build differs from your local build.

Test it: `brew audit --cask --strict roofle-ai/roofle/roofle`, then
`brew install --cask roofle-ai/roofle/roofle`.

Users install and update with:
```sh
brew install --cask roofle-ai/roofle/roofle
brew upgrade --cask roofle
```

Never re-upload a different DMG under a version that's already published; the
checksum in the cask would no longer match. Cut a new version instead.

## 5. Troubleshooting

- **"Roofle is damaged" / Gatekeeper warning:** the build wasn't signed and
  notarized. Re-check step 4 and that the environment variables were set in the
  same shell.
- **Notarization rejected:** run `xcrun notarytool log <submission-id>
  --apple-id … --team-id …` to see why. Usually a missing hardened runtime or
  an unsigned nested binary.
- **Microphone silent in the signed build:** the entitlements file is missing
  (see above).
- **Wrong or old icon in System Settings:** macOS looks icons up by bundle ID
  (`com.roofle.app`), and old copies of the app (DMG volumes, `target/` builds,
  an earlier Electron build) can still be registered. Delete stale copies, then:
  ```sh
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -kill -r -domain local -domain user
  killall Dock Finder
  ```
  and remove any stale Roofle row in Privacy & Security → Screen & System Audio
  Recording, then re-add the freshly installed app.
- **Permission prompts reappear after every rebuild:** the build isn't signed
  with a stable Developer ID identity, so macOS treats each build as a new app.
