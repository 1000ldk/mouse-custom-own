#!/bin/sh
# Mac 用のアプリ (pinch-zoom.app) を組み立てて zip にする。
#
#   cargo build --release                  # いまの Mac 用だけ
#   ./macos/bundle.sh
#
# Apple Silicon と Intel の両方で動くアプリにするには、先に両方をビルドしておく:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   cargo build --release --target aarch64-apple-darwin
#   cargo build --release --target x86_64-apple-darwin
#   ./macos/bundle.sh
#
# できるもの: target/macos/pinch-zoom.app と target/macos/pinch-zoom-macos.zip
set -eu

cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
out=target/macos
app="$out/pinch-zoom.app"
arm=target/aarch64-apple-darwin/release/pinch-zoom
intel=target/x86_64-apple-darwin/release/pinch-zoom

rm -rf "$out"
mkdir -p "$app/Contents/MacOS"

if [ -f "$arm" ] && [ -f "$intel" ]; then
    # 2 つの実行ファイルを 1 つにまとめる (ユニバーサルバイナリ)
    lipo -create -output "$app/Contents/MacOS/pinch-zoom" "$arm" "$intel"
else
    cp target/release/pinch-zoom "$app/Contents/MacOS/pinch-zoom"
fi
sed "s/@VERSION@/$version/g" macos/Info.plist > "$app/Contents/Info.plist"

# 署名 (Apple の証明書を使わない「アドホック署名」)。Apple Silicon では署名の無いアプリは起動できない
codesign --force --sign - "$app"

(cd "$out" && ditto -c -k --keepParent pinch-zoom.app pinch-zoom-macos.zip)
echo "$out/pinch-zoom-macos.zip"
