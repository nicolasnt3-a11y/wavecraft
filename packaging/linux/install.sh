#!/bin/sh
# Installation de Wavecraft sous Linux.
#
# Le script explique chaque geste avant de le faire et demande l'accord. Rien
# n'est écrit en silence : il s'agit de régler des droits d'accès au matériel,
# pas de prendre la main sur le système.
#
# Usage : sudo ./install.sh [--prefix /opt/wavecraft] [--oui]

set -eu

PREFIX=/opt/wavecraft
BIN=/usr/local/bin/wavecraft
REGLE=/etc/udev/rules.d/60-wavecraft-rtlsdr.rules
BLACKLIST=/etc/modprobe.d/wavecraft-rtlsdr.conf
SOURCE=$(cd "$(dirname "$0")" && pwd)
TOUT_ACCEPTER=0

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX=$2; shift 2 ;;
        --oui|-y) TOUT_ACCEPTER=1; shift ;;
        --aide|-h)
            echo "Usage : sudo ./install.sh [--prefix /opt/wavecraft] [--oui]"
            exit 0 ;;
        *) echo "Option inconnue : $1" >&2; exit 2 ;;
    esac
done

if [ "$(id -u)" -ne 0 ]; then
    echo "Ce script doit être lancé avec sudo." >&2
    exit 1
fi

demander() {
    if [ "$TOUT_ACCEPTER" -eq 1 ]; then
        return 0
    fi
    printf '%s [o/N] ' "$1"
    read -r reponse
    case "$reponse" in
        o|O|oui|OUI) return 0 ;;
        *) return 1 ;;
    esac
}

echo "Wavecraft — installation"
echo

# 1. L'application ------------------------------------------------------------
if [ ! -f "$SOURCE/wavecraft" ]; then
    echo "L'exécutable « wavecraft » est introuvable à côté de ce script." >&2
    exit 1
fi

echo "1. Copier l'application dans $PREFIX et créer le raccourci $BIN."
if demander "   Faire cela ?"; then
    install -d "$PREFIX"
    install -m 755 "$SOURCE/wavecraft" "$PREFIX/wavecraft"
    [ -f "$SOURCE/librtlsdr.so" ] && install -m 644 "$SOURCE/librtlsdr.so" "$PREFIX/"
    ln -sf "$PREFIX/wavecraft" "$BIN"
    echo "   fait."
else
    echo "   ignoré."
fi
echo

# 2. Accès à la clé -----------------------------------------------------------
echo "2. Autoriser l'accès à la clé RTL-SDR sans être administrateur."
echo "   Écrit $REGLE :"
echo "   une règle udev qui donne l'accès à la clé à l'utilisateur connecté."
if demander "   Faire cela ?"; then
    cp "$SOURCE/60-wavecraft-rtlsdr.rules" "$REGLE"
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger 2>/dev/null || true
    echo "   fait. Débranchez et rebranchez la clé pour que la règle s'applique."
else
    echo "   ignoré — Wavecraft devra être lancé avec sudo."
fi
echo

# 3. Pilote de télévision numérique -------------------------------------------
echo "3. Écarter le pilote de télévision numérique (dvb_usb_rtl28xxu)."
echo "   Ce pilote prend la clé à son branchement et empêche toute écoute."
echo "   Écrit $BLACKLIST, puis le décharge s'il est en mémoire."
if demander "   Faire cela ?"; then
    cp "$SOURCE/wavecraft-rtlsdr.conf" "$BLACKLIST"
    modprobe -r dvb_usb_rtl28xxu 2>/dev/null || true
    echo "   fait."
else
    echo "   ignoré — si vous n'entendez rien, c'est la première chose à refaire."
fi
echo

# 4. Service (facultatif) -----------------------------------------------------
echo "4. Mode serveur au démarrage de la machine (facultatif)."
echo "   Installe le service systemd wavecraft.service, qui lance"
echo "   « wavecraft --server --bind 0.0.0.0 --port 8073 »."
if demander "   Installer le service ?"; then
    cp "$SOURCE/wavecraft.service" /etc/systemd/system/wavecraft.service
    systemctl daemon-reload
    echo "   installé. Pour le démarrer : systemctl enable --now wavecraft"
    echo "   Pensez à un mot de passe si le réseau n'est pas de confiance :"
    echo "   éditez /etc/systemd/system/wavecraft.service et ajoutez --password."
else
    echo "   ignoré."
fi
echo

echo "Terminé. Lancez « wavecraft » pour ouvrir la fenêtre,"
echo "ou « wavecraft --probe » pour un diagnostic."
