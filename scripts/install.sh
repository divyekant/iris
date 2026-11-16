#!/bin/sh
# Install Iris: download the iris program for this system from the latest
# GitHub Release, check its SHA-256 sum, and install it to ~/.local/bin.
# No sudo. The script connects only to the download address below.
#
#   curl -fsSLO https://raw.githubusercontent.com/divyekant/iris/main/scripts/install.sh && sh install.sh
#
# Settings (environment variables):
#   IRIS_VERSION       a release tag, for example v2026.11.16.1. Default: the latest release.
#   IRIS_INSTALL_DIR   the folder for the program. Default: ~/.local/bin.
#   IRIS_NO_SERVICE=1  do not install the background service.
#   IRIS_DOWNLOAD_URL  the folder that holds the release files (for tests).
set -eu

repo="https://github.com/divyekant/iris"

fail() {
    echo "iris install: $*" >&2
    exit 1
}

case "$(uname -s)" in
    Darwin) os=apple-darwin ;;
    Linux) os=unknown-linux-musl ;;
    *) fail "this system ($(uname -s)) is not supported. Build from source: $repo" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) cpu=x86_64 ;;
    arm64 | aarch64) cpu=aarch64 ;;
    *) fail "this CPU ($(uname -m)) is not supported. Build from source: $repo" ;;
esac
asset="iris-$cpu-$os.tar.gz"

if [ -n "${IRIS_DOWNLOAD_URL:-}" ]; then
    base="$IRIS_DOWNLOAD_URL"
elif [ -n "${IRIS_VERSION:-}" ]; then
    base="$repo/releases/download/$IRIS_VERSION"
else
    base="$repo/releases/latest/download"
fi

if command -v sha256sum >/dev/null 2>&1; then
    sum() { sha256sum "$1" | cut -d ' ' -f 1; }
elif command -v shasum >/dev/null 2>&1; then
    sum() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
else
    fail "sha256sum or shasum is necessary to check the download"
fi
command -v curl >/dev/null 2>&1 || fail "curl is necessary to download Iris"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM

echo "iris install: download $asset"
curl -fsSL -o "$work/$asset" "$base/$asset" || fail "cannot download $base/$asset"
curl -fsSL -o "$work/SHA256SUMS" "$base/SHA256SUMS" || fail "cannot download $base/SHA256SUMS"

want="$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1 }' "$work/SHA256SUMS")"
[ -n "$want" ] || fail "SHA256SUMS has no line for $asset"
got="$(sum "$work/$asset")"
[ "$got" = "$want" ] || fail "the SHA-256 sum of $asset is wrong (got $got, want $want). Nothing is installed."

tar -xzf "$work/$asset" -C "$work"
[ -f "$work/iris" ] || fail "$asset has no iris program"

dir="${IRIS_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$dir"
# Move into place in one step, so that a running iris keeps its old file.
cp "$work/iris" "$dir/.iris.new"
chmod 755 "$dir/.iris.new"
mv -f "$dir/.iris.new" "$dir/iris"
echo "iris install: installed $dir/iris"

case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "iris install: add $dir to PATH, for example: export PATH=\"$dir:\$PATH\"" ;;
esac

if [ "${IRIS_NO_SERVICE:-}" = 1 ]; then
    echo "iris install: the background service is not installed (IRIS_NO_SERVICE=1)"
elif "$dir/iris" help service >/dev/null 2>&1; then
    # A system with no service manager (for example, Linux with no systemd)
    # keeps the program, and the user starts the engine by hand.
    "$dir/iris" service install || {
        echo "iris install: the program is installed, but the background service did not start (see the error above)." >&2
        echo "iris install: start the engine with: iris serve" >&2
    }
else
    echo "iris install: this version of iris has no service command. Start the engine with: iris serve"
fi
