#!/bin/bash
set -Eeuo pipefail

case $(uname -m) in
    x86_64) arch="amd64" ;;
    aarch64) arch="arm64" ;;
    *) echo "Unkown arch" ; uname -m  ;;
esac

sudo rm -rf target/debian
mkdir target/debian
mkdir -p target/debian/usr/bin target/debian/DEBIAN
mkdir -p target/debian/usr/share/man/man1
mkdir -p target/debian/usr/share/doc/hurl
mkdir -p target/debian/usr/share/applications

cp target/release/hurl target/release/hurlfmt target/release/hurlui target/debian/usr/bin
for size in 32x32 128x128 ; do
    mkdir -p "target/debian/usr/share/icons/hicolor/${size}/apps"
    cp "packages/hurlui/icons/${size}.png" "target/debian/usr/share/icons/hicolor/${size}/apps/hurlui.png"
done
cat >target/debian/usr/share/applications/hurlui.desktop <<END
[Desktop Entry]
Type=Application
Name=Hurl UI
Comment=Desktop client for Hurl
Exec=hurlui
Icon=hurlui
StartupWMClass=hurlui
Terminal=false
Categories=Development;
END
cp target/man/hurl.1.gz target/man/hurlfmt.1.gz target/debian/usr/share/man/man1
gzip -9 -n --stdout CHANGELOG.md > target/debian/usr/share/doc/hurl/changelog.Debian.gz
cat >target/debian/usr/share/doc/hurl/copyright <<END
Files: *
Copyright: 2023, Orange
License: http://www.apache.org/licenses/LICENSE-2.0
END

sudo chown -R root:root target/debian/usr

cat <<END >target/debian/DEBIAN/control
Package: hurl
Version: $VERSION
Section: web
Architecture: $arch
Priority: optional
Standards-Version: 3.9.4
Maintainer: Fabrice Reix <fabrice.reix@orange.com>
Depends:  libc6 (>= 2.17), libcurl4, zlib1g, libxml2
Recommends: libgtk-3-0, libwebkit2gtk-4.1-0
Description: Run and test HTTP requests
 Hurl is an HTTP client that performs HTTP requests defined in a simple plain
 text format.
 Hurl is very versatile, it enables to chain HTTP requests, capture values
 from HTTP responses and make asserts.
 This package also provides hurlfmt, and hurlui, a desktop client for Hurl:
 hurlui needs the recommended packages, hurl and hurlfmt do not.

END
dpkg --build target/debian

echo "Check Lintian"
sudo apt-get install -y lintian
lintian --verbose target/debian.deb

mkdir -p target/upload
cp target/debian.deb "target/upload/hurl_${VERSION}_${arch}.deb"

