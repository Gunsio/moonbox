# Homebrew Release Notes

Moonbox prereleases are distributed through a dedicated Homebrew tap:

```bash
brew tap Gunsio/tap
brew trust --formula gunsio/tap/moonbox
brew install moonbox
```

Homebrew 5 requires explicit trust for third-party taps. Prefer trusting only
the `gunsio/tap/moonbox` formula rather than the whole tap. On Apple Silicon
macOS, the published formula pours a tagged bottle by default so users do not
need Rust, LLVM, or current Apple Command Line Tools for the default install
path. Source builds are only a fallback for architectures that do not yet have
binary artifacts.

The tap formula is pinned to tagged GitHub release artifacts, not to an
unversioned branch or GitHub's auto-generated archive. The workflow follows the
current Homebrew guidance for stable source archives, checksums,
`std_cargo_args`, and executable-generated completions:

- [Homebrew Formula Cookbook](https://docs.brew.sh/Formula-Cookbook)
- [Homebrew Formula API](https://docs.brew.sh/rubydoc/Formula.html)

Homebrew Core can come later, after the project has stable releases, external
usage, and a lower-risk dependency/release profile.

## Release Checklist

1. Merge the accepted release milestone PR.
2. Tag the accepted commit, for example `v0.1.5-beta.54`.
3. Stage local release artifacts:

```bash
scripts/release/stage-artifacts.sh --version 0.1.5-beta.54 --tag v0.1.5-beta.54
```

4. Create a GitHub prerelease and upload the staged source archive, Cargo crate
   archive, host binary archive,
   `SHA256SUMS`, and `release-manifest.json` to the GitHub release.
5. Build and upload Apple Silicon Homebrew bottles for the supported macOS
   bottle tags, then add the bottle checksums to `Gunsio/homebrew-tap`'s
   `Formula/moonbox.rb`.
6. Copy the `homebrew.sha256` value from `release-manifest.json` into the source
   fallback in `Gunsio/homebrew-tap`'s `Formula/moonbox.rb`.
7. Run formula verification:

```bash
brew audit --strict --formula moonbox
brew test moonbox
```

8. Verify the public install path from a clean tap checkout:

```bash
brew tap Gunsio/tap
brew trust --formula gunsio/tap/moonbox
brew install moonbox
moon --version
```

## Local Dry-Run

Before publishing or updating the tap, run the repository-level Homebrew formula
smoke:

```bash
scripts/ci/homebrew-docs-smoke.sh
```

The smoke checks the formula template syntax, verifies that it still uses the
expected Cargo and completion helpers, and runs the same completion-generation
commands against the built `moonbox` and `moon` binaries. It redirects source
homes into `target/moonbox-homebrew-smoke-home`, sets
`MOONBOX_SESSION_MODE=fixture`, and does not open, resume, or discover real
sessions.

Run the release artifact smoke before tagging or uploading:

```bash
scripts/ci/release-artifacts-smoke.sh
```

The smoke stages artifacts under `target/`, validates the generated checksums,
checks the JSON manifest, and verifies that the source, Cargo crate, and host
binary archives contain the expected files. It sets `MOONBOX_SESSION_MODE=fixture`
while generating shell completions for the binary archive and does not open,
resume, or discover real sessions.

## Formula Shape

The source repository keeps a formula template at
[homebrew/moonbox.rb](homebrew/moonbox.rb). The published formula lives in
`Gunsio/homebrew-tap` as `Formula/moonbox.rb` and must pin tagged release
artifacts. Apple Silicon macOS should use published bottle checksums first;
source fallbacks should use `release-manifest.json`'s `homebrew.sha256` field
for the source archive.

```ruby
class Moonbox < Formula
  desc "Cross-CLI session rewind workbench"
  homepage "https://github.com/Gunsio/moonbox"
  license "MIT"

  bottle do
    root_url "https://github.com/Gunsio/moonbox/releases/download/v0.1.5-beta.54"
    rebuild 1
    sha256 cellar: :any_skip_relocation, arm64_tahoe: "<release-bottle-sha256>"
    sha256 cellar: :any_skip_relocation, arm64_sequoia: "<release-bottle-sha256>"
  end

  on_macos do
    on_arm do
      url "https://github.com/Gunsio/moonbox/releases/download/v0.1.5-beta.54/moonbox-0.1.5-beta.54-aarch64-apple-darwin.tar.gz"
      sha256 "<release-binary-sha256>"
    end

    on_intel do
      url "https://github.com/Gunsio/moonbox/releases/download/v0.1.5-beta.54/moonbox-0.1.5-beta.54-source.tar.gz"
      sha256 "<release-source-sha256>"

      depends_on "rust" => :build
    end
  end

  def install
    binary_root = if (buildpath/"bin/moonbox").exist?
      buildpath
    elsif (buildpath/"moonbox-0.1.5-beta.54-aarch64-apple-darwin/bin/moonbox").exist?
      buildpath/"moonbox-0.1.5-beta.54-aarch64-apple-darwin"
    end

    if binary_root
      bin.install binary_root/"bin/moonbox", binary_root/"bin/moon"
    else
      system "cargo", "install", *std_cargo_args
    end

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
