# Homebrew Release Notes

Moonbox can be distributed through Homebrew, but the formula should not be
published until a release is accepted and tagged.

This plan follows the current Homebrew guidance for stable source archives,
checksums, `std_cargo_args`, and executable-generated completions:

- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [Homebrew Formula API](https://docs.brew.sh/rubydoc/Formula.html)

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
brew audit --strict --formula moonbox
brew test moonbox
```

7. Update the README installation section from "planned" to the published tap
   command.

## Local Dry-Run

Before publishing, run the repository-level Homebrew docs smoke:

```bash
scripts/ci/homebrew-docs-smoke.sh
```

The smoke checks the draft formula syntax, verifies that it still uses the
expected Cargo and completion helpers, and runs the same completion-generation
commands against the built `moonbox` and `moon` binaries. It redirects source
homes into `target/moonbox-homebrew-smoke-home` and does not open or resume
real sessions.

## Formula Shape

The draft formula lives at [homebrew/moonbox.rb](homebrew/moonbox.rb). The first
published formula should pin a released archive, not build from an unversioned
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
    generate_completions_from_executable(bin/"moonbox", "completions", shells: [:bash, :zsh, :fish, :pwsh])
    generate_completions_from_executable(bin/"moon", "completions", shells: [:bash, :zsh, :fish, :pwsh])
  end

  test do
    assert_match "moonbox", shell_output("#{bin}/moonbox --version")
    assert_match "moonbox", shell_output("#{bin}/moon --version")
    assert_match "fixture_only", shell_output("#{bin}/moonbox replay-eval --json")
    assert_match "replay-eval", shell_output("#{bin}/moonbox completions bash")
    assert_match "complete -c moon", shell_output("#{bin}/moon completions fish")
    assert_match "Register-ArgumentCompleter", shell_output("#{bin}/moon completions powershell")
  end
end
```
