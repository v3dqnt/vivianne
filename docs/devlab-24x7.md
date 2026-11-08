# Running Vivianne 24/7 on a Nebius Devlab

A CPU Devlab is about $0.08/hr (about $58/month). Model calls go to Token Factory per token, so no GPU is needed for the always-on agent. Use a GPU Devlab ($1.57/hr) or a Job only for heavy work, and stop it after.

## What works today
`--headless` (or `VIV_HEADLESS=1`) runs the same app with nothing shown: the main window is hidden, and the notch and overlays are never created. Agents, routines and bots keep running. The agent's model calls still run in the app's webview, so a display is needed: use a virtual one.

## What does not exist yet
- **Remote control.** There is no way yet to drive the Devlab instance from your desktop app. Today you reach it through its channels (bots) and by SSH. A bearer-authenticated remote client is the next piece (NemoClaw plan, phase C).
- **A Linux build.** The app has been verified on Windows only. Expect to fix things on first run.
- **Approvals while away.** Anything that needs approval waits; there is no push to your phone yet.

## Setup (Ubuntu Devlab)
Fastest: copy `scripts/devlab-setup.sh` to the box and run it. It installs the packages below and the official `nebius` CLI (Ubuntu and macOS are the only platforms Nebius documents), then tells you the two sign-in steps it can't do for you: `nebius profile create`, and `nebius ai job list` to confirm jobs work. With the CLI there, `nebius_job` works on the Devlab instance.

By hand:
```bash
sudo apt-get update
sudo apt-get install -y xvfb libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1 libsecret-1-0 gnome-keyring dbus-x11

# Build on a machine with the toolchain (or in the Devlab): cargo tauri build, then copy the .deb/.AppImage here.
sudo dpkg -i vivianne_*.deb

# One-off: sign in and add your provider key (Nebius Token Factory) in the app's settings,
# either via a desktop session once, or by copying ~/.viv and the keychain entries.
```

Keep it running with systemd (user unit `~/.config/systemd/user/vivianne.service`):
```ini
[Unit]
Description=Vivianne (headless)
After=network-online.target

[Service]
Environment=VIV_HEADLESS=1
ExecStart=/usr/bin/xvfb-run -a /usr/bin/vivianne --headless
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
```
```bash
systemctl --user daemon-reload && systemctl --user enable --now vivianne
loginctl enable-linger "$USER"   # keep it running after you log out
journalctl --user -u vivianne -f
```

## Cost guard
- Name the Devlab clearly and set a budget alert in the Nebius console.
- Stop GPU Devlabs when idle; the CPU one is the only one meant to stay on.
