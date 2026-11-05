cask "roofle" do
  version "1.0.0-beta"
  sha256 "21720b5a5cb7e98dbb5127e4052f1e2c8428798b7a76db86558f442d7c432e48"

  url "https://github.com/roofle-ai/roofle/releases/download/v#{version}/Roofle_#{version}_aarch64.dmg"
  name "Roofle"
  desc "Real-time meeting transcription, notes and suggested questions"
  homepage "https://github.com/roofle-ai/roofle"

  # The DMG is built for Apple Silicon; system-audio capture needs ScreenCaptureKit.
  depends_on arch: :arm64
  depends_on macos: ">= :ventura"

  app "Roofle.app"

  zap trash: [
    "~/Library/Application Support/com.roofle.app",
    "~/Library/Caches/com.roofle.app",
    "~/Library/Preferences/com.roofle.app.plist",
    "~/Library/Saved Application State/com.roofle.app.savedState",
    "~/Library/WebKit/com.roofle.app",
  ]
end
