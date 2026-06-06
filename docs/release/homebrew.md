# Homebrew Release Notes

Moonbox can be distributed through Homebrew, but the formula should not be
published until a release is accepted and tagged.

## Preferred Path

Use a dedicated tap first:

```bash
brew tap Gunsio/tap
brew install moonbox
```

Homebrew Core can come later, after the project has stable releases, external
usage, and a lower-risk dependency/release profile.

## Release Checklist

1. Merge the accepted milestone PR.
2. Tag a version, for example `v0.1.0`.
3. Build and attach release archives for supported platforms.
4. Generate SHA-256 checksums for each archive.
5. Add or update the formula in `Gunsio/homebrew-tap`.
6. Run formula verification:

```bash
brew audit --strict moonbox
brew test moonbox
```

7. Update the README installation section from "planned" to the published tap
   command.

## Formula Shape

The first formula should pin a released archive, not build from an unversioned
Git branch. The formula must include a checksum.

```ruby
class Moonbox < Formula
  desc "Cross-CLI session rewind workbench"
  homepage "https://github.com/Gunsio/moonbox"
  url "https://github.com/Gunsio/moonbox/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "<release-archive-sha256>"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "moonbox", shell_output("#{bin}/moonbox --version")
    assert_match "moonbox", shell_output("#{bin}/moon --version")
  end
end
```
