#!/usr/bin/env bash
# Build and package BIMCraft for Linux (<arch> is x86_64 or aarch64, the host's, or riscv64 cross-built):
#
#   $DIST/bimcraft-<version>-linux-<arch>.AppImage        any distro with glibc >= the build host's
#   $DIST/bimcraft-<version>-linux-<arch>.AppImage.zsync  its delta updates (needs zsyncmake)
#   $DIST/bimcraft-<version>-linux-<arch>.deb             Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/bimcraft-<version>-linux-<arch>.rpm             Fedora, openSUSE, RHEL, ...
#   $DIST/bimcraft-<version>-linux-<arch>.tar.gz          a plain tree (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"]
#
# Needs: cargo; nfpm for the deb and the rpm (https://nfpm.goreleaser.com); appimagetool for the
# AppImage (downloaded into $CARGO_TARGET_DIR if missing). Build on an old distro (CI: Ubuntu
# 22.04, glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate and
# appstreamcli check the metadata; zsyncmake (the zsync package) writes the AppImage's .zsync.
#
# riscv64 is cross-built, and only as deb, rpm and tar (appimagetool has no riscv64 runtime). Set
# CROSS_ARCH=riscv64, CROSS_TARGET=riscv64gc-unknown-linux-gnu and CROSS_COMPILE=riscv64-linux-gnu-
# (the binutils prefix), and EMULATOR=qemu-riscv64 (with QEMU_LD_PREFIX at the cross sysroot) to run
# the result once. CI does it on Ubuntu 24.04, so the packages need glibc 2.39 or later.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=io.github.ipedrax.bimcraft

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="${CROSS_ARCH:-$(uname -m)}"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  riscv64) DEB_ARCH=riscv64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
case " $FORMATS " in
  *" appimage "*) [ -z "${CROSS_TARGET:-}" ] || { echo "error: no AppImage for a cross build: --formats \"deb rpm tar\"" >&2; exit 2; } ;;
esac
export BIMCRAFT_MAINTAINER="${BIMCRAFT_MAINTAINER:-IPedrax <https://github.com/IPedrax/BIMCraft>}"
BASENAME="bimcraft-$VERSION-linux-$ARCH"

echo "==> BIMCraft $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  if [ -n "${CROSS_TARGET:-}" ]; then
    (cd "$ROOT" && cargo build --release --locked -p bimcraft -p bimcraft-cli --target "$CROSS_TARGET")
  else
    (cd "$ROOT" && cargo build --release --locked -p bimcraft -p bimcraft-cli)
  fi
fi
BIN="$CARGO_TARGET_DIR/${CROSS_TARGET:+$CROSS_TARGET/}release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- a tree every format is made from ----------------------------------------------------------
install -Dm755 "$BIN/bimcraft" "$STAGE/usr/bin/bimcraft"
install -Dm755 "$BIN/bimcraft-cli" "$STAGE/usr/bin/bimcraft-cli"
"${CROSS_COMPILE:-}strip" "$STAGE/usr/bin/bimcraft" "$STAGE/usr/bin/bimcraft-cli"
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$HERE/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
mkdir -p "$STAGE/usr/share/doc/bimcraft"
copy_docs "$STAGE/usr/share/doc/bimcraft"

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz -----------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb and .rpm -----------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export NFPM_ARCH="$DEB_ARCH"
  # nfpm expands environment variables in fields like `version` and `arch`, but not in `src`.
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  # Built on Ubuntu 24.04, the riscv64 binaries need glibc 2.39; the others, built on 22.04, 2.35.
  [ "$ARCH" != riscv64 ] || sed -i -e 's/libc6 (>= 2.35)/libc6 (>= 2.39)/' -e 's/libstdc++6 (>= 12)/libstdc++6 (>= 13)/' "$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage ----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/BIMCraft.AppDir"
  # The docs stay: the licences, the notice and the fonts' OFL go with every copy of the executable that holds the fonts.
  cp -R "$STAGE" "$APPDIR"
  ln -s usr/bin/bimcraft "$APPDIR/AppRun"
  cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [ -z "$TOOL" ]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [ ! -x "$TOOL" ]; then
      curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  OUT="$DIST/$BASENAME.AppImage"
  # A .zsync left by an earlier run would hide a missing zsyncmake and describe another file.
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate, AppImageLauncher and the like read it from the file and
  # fetch only the blocks that changed in a newer release, through the .zsync published beside
  # each AppImage. `latest` is the newest release that isn't a pre-release; a fork's builds
  # point at its own releases through GITHUB_REPOSITORY.
  REPO="${GITHUB_REPOSITORY:-IPedrax/BIMCraft}"
  UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|bimcraft-*-linux-$ARCH.AppImage.zsync"
  # Extract-and-run works without FUSE (containers, CI); the AppImage it writes embeds the static
  # runtime, so users don't need libfuse2 either. appimagetool writes the .zsync, when zsyncmake
  # is there, into the directory it runs in, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream -u "$UPDATE_INFO" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [ -s "$OUT.zsync" ]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

if [ -z "${CROSS_TARGET:-}" ]; then
  "$STAGE/usr/bin/bimcraft-cli" --version
  "$STAGE/usr/bin/bimcraft" --version
elif [ -n "${EMULATOR:-}" ]; then
  "$EMULATOR" "$STAGE/usr/bin/bimcraft-cli" --version
  "$EMULATOR" "$STAGE/usr/bin/bimcraft" --version
fi
echo "==> done"
ls -lh "$DIST"
