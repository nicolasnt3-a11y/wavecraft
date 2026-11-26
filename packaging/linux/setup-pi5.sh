#!/usr/bin/env bash
# Installation complète et automatique de Wavecraft (mode serveur) sur Raspberry Pi 5
# (Raspberry Pi OS 64 bits / Debian / Ubuntu ARM64 ou x64).
#
# Usage sur le Raspberry Pi :
#   curl -fsSL https://raw.githubusercontent.com/nicolasnt3-a11y/wavecraft/main/packaging/linux/setup-pi5.sh | sudo bash
#
# Optionnel : définir un mot de passe pour l'interface web :
#   WAVECRAFT_PASSWORD="monmotdepasse" curl -fsSL ... | sudo -E bash

set -euo pipefail

REPO="https://github.com/nicolasnt3-a11y/wavecraft.git"
RTL_REPO="https://github.com/rtlsdrblog/rtl-sdr-blog.git"
BUILD_DIR="/tmp/wavecraft-build"
PREFIX="/opt/wavecraft"
PORT="${WAVECRAFT_PORT:-8073}"
PASSWORD="${WAVECRAFT_PASSWORD:-}"

if [ "$(id -u)" -ne 0 ]; then
    echo "Erreur : lancez ce script avec sudo." >&2
    exit 1
fi

echo "=== 1/5 : Dépendances système ==="
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq git curl build-essential cmake pkg-config libusb-1.0-0-dev ca-certificates

echo "=== 2/5 : Pilote officiel RTL-SDR Blog V4 (librtlsdr) ==="
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
git clone --depth 1 "$RTL_REPO" "$BUILD_DIR/rtl-sdr-blog"
cmake -S "$BUILD_DIR/rtl-sdr-blog" -B "$BUILD_DIR/rtl-sdr-blog/build" \
    -DINSTALL_UDEV_RULES=ON -DDETACH_KERNEL_DRIVER=ON
cmake --build "$BUILD_DIR/rtl-sdr-blog/build" -j"$(nproc)"
cmake --install "$BUILD_DIR/rtl-sdr-blog/build"
ldconfig

echo "=== 3/5 : Compilation de Wavecraft (mode serveur) ==="
if ! command -v cargo >/dev/null 2>&1; then
    if [ -x /root/.cargo/bin/cargo ]; then
        export PATH="/root/.cargo/bin:$PATH"
    else
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
        export PATH="/root/.cargo/bin:$PATH"
    fi
fi

git clone --depth 1 "$REPO" "$BUILD_DIR/wavecraft"
cd "$BUILD_DIR/wavecraft/app"
cargo build --release --no-default-features --features server

echo "=== 4/5 : Installation de Wavecraft, udev et blacklist DVB-T ==="
install -d "$PREFIX"
install -m 755 "$BUILD_DIR/wavecraft/app/target/release/wavecraft" "$PREFIX/wavecraft"
ln -sf "$PREFIX/wavecraft" /usr/local/bin/wavecraft

cp "$BUILD_DIR/wavecraft/packaging/linux/60-wavecraft-rtlsdr.rules" /etc/udev/rules.d/60-wavecraft-rtlsdr.rules
cp "$BUILD_DIR/wavecraft/packaging/linux/wavecraft-rtlsdr.conf" /etc/modprobe.d/wavecraft-rtlsdr.conf
udevadm control --reload-rules || true
udevadm trigger || true
modprobe -r dvb_usb_rtl28xxu 2>/dev/null || true

echo "=== 5/5 : Configuration du service systemd ==="
cp "$BUILD_DIR/wavecraft/packaging/linux/wavecraft.service" /etc/systemd/system/wavecraft.service
EXEC_LINE="/opt/wavecraft/wavecraft --server --bind 0.0.0.0 --port ${PORT}"
if [ -n "$PASSWORD" ]; then
    EXEC_LINE="${EXEC_LINE} --password ${PASSWORD}"
fi
sed -i "s|^ExecStart=.*|ExecStart=${EXEC_LINE}|" /etc/systemd/system/wavecraft.service

systemctl daemon-reload
systemctl enable --now wavecraft

rm -rf "$BUILD_DIR"

IP=$(hostname -I | awk '{print $1}')
HOST=$(hostname)
echo
echo "============================================================"
echo " Wavecraft est installé et actif sur votre Raspberry Pi !"
echo " Adresse locale : http://${HOST}.local:${PORT}/"
if [ -n "$IP" ]; then
echo " Adresse IP     : http://${IP}:${PORT}/"
fi
echo " Diagnostic     : wavecraft --probe"
echo " Journaux       : journalctl -u wavecraft -f"
echo "============================================================"
echo "Note : redémarrez une fois (sudo reboot) si la clé RTL-SDR"
echo "était déjà branchée avant l'installation."
