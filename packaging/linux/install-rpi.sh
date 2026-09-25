#!/usr/bin/env bash
# SterOidSoundBoard: install as a headless service on Raspberry Pi OS (64-bit) / Debian.
set -euo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
RUN_USER="${SUDO_USER:-$USER}"
sudo install -m755 "$DIR/steroidsoundboard" /usr/local/bin/steroidsoundboard
sed "s/^User=.*/User=$RUN_USER/" "$DIR/steroidsoundboard.service" | sudo tee /etc/systemd/system/steroidsoundboard.service >/dev/null
sudo usermod -aG audio "$RUN_USER"
# Real-time limits for the audio group
echo -e "@audio - rtprio 95\n@audio - memlock unlimited" | sudo tee /etc/security/limits.d/95-steroidsoundboard.conf >/dev/null
# CPU governor -> performance (avoids clock scaling glitches)
if [ -w /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ] || sudo true; then
  echo 'GOVERNOR="performance"' | sudo tee /etc/default/cpufrequtils >/dev/null || true
  for g in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do echo performance | sudo tee "$g" >/dev/null || true; done
fi
sudo systemctl daemon-reload
sudo systemctl enable --now steroidsoundboard
echo "SterOidSoundBoard running: http://$(hostname -I | awk '{print $1}'):8420"
