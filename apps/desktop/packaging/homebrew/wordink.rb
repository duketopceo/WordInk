cask "wordink" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "@URL@"
  name "WordInk"
  desc "Local voice dictation"
  homepage "https://github.com/duketopceo/WordInk"

  # livecheck is wired in U14 once a release manifest exists.

  auto_updates true
  depends_on arch: :arm64
  depends_on macos: :sequoia

  app "WordInk.app"

  uninstall quit: "io.github.duketopceo.wordink"

  zap trash: [
    "~/.config/wordink",
    "~/Library/Application Support/wordink",
    "~/Library/Caches/io.github.duketopceo.wordink",
    "~/Library/HTTPStorages/io.github.duketopceo.wordink",
    "~/Library/Preferences/io.github.duketopceo.wordink.plist",
    "~/Library/Saved Application State/io.github.duketopceo.wordink.savedState",
  ]
end
