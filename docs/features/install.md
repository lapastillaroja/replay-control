# Installation

Replay Control runs alongside RePlayOS on a Raspberry Pi. You can install it from the Pi or from another computer on the same network.

## Install on the Pi

On Windows 10+, macOS, or Linux, open PowerShell, Terminal, or another shell and connect to the Pi:

```sh
ssh root@replay.local
# default password: replayos
```

If SSH asks whether to trust the host, type `yes`. Your password will not appear while you type it. Once the Pi shell opens, run:

```sh
curl -fsSL \
  https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh |
  bash
```

When installation finishes, open **https://replay.local:8443** in your browser. Approve the certificate exception on your first visit.

If `replay.local` does not work, use the Pi's IP address for SSH and the browser. You can find it in your router's connected devices list.

## Install from another computer

On Linux or macOS, the installer can connect to the Pi for you. Run this on a computer on the same network:

```sh
curl -fsSL \
  https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh |
  bash
```

It uses the Pi's root password, which is `replayos` on a fresh RePlayOS image. If the Pi's name cannot be found, provide its IP address:

```sh
curl -fsSL \
  https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh |
  bash -s -- --ip 192.168.1.50
```

On Windows, use the SSH steps above from PowerShell or Windows Terminal.

## Update

Use **Settings > Updates > Update Now** in Replay Control, or run the installation command again. Replay Control updates independently of RePlayOS. Your settings and game library are preserved.

## Remove

To remove Replay Control while keeping its settings and library data, connect to the Pi:

```sh
ssh root@replay.local
# default password: replayos
```

Then run:

```sh
curl -fsSL \
  https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh |
  bash -s -- --uninstall
```

**Purge deletes your Replay Control settings, library databases, and downloaded media.** You will lose app preferences and library data, and Replay Control will need to rebuild or download that data if you reinstall it. Your ROMs, saves, captures, and BIOS files are kept. If you need the Replay Control data later, back it up before running:

```sh
curl -fsSL \
  https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh |
  bash -s -- --purge
```

The installer lists the paths it will delete and asks you to confirm. For an unattended purge, add `--yes` to skip that prompt.

## Other options

Use `--version VERSION` to install a specific release, `--pi-pass PASSWORD` if you changed the Pi password, or `--dry-run` to preview the install. Add these after `bash -s --` in the commands above.

## Standalone use

Replay Control can run without RePlayOS on **Debian 13 (amd64 or arm64)** and **Fedora 42 (x86_64)**. Standalone mode is **work in progress and unsupported**.

Download the package for your system from a [Replay Control release](https://github.com/lapastillaroja/replay-control/releases), then install the downloaded file:

```sh
# Debian 13: use the amd64 or arm64 .deb for your computer
sudo apt install ./replay-control_VERSION_ARCH.deb

# Fedora 42, x86_64
sudo dnf install ./replay-control-VERSION-1.x86_64.rpm
```

Replace the example filename with the exact file you downloaded. Open **https://localhost:8443** on that computer. On first use, choose the folder that contains your `roms/` directory. You can change it later in **Settings > Library & Games > Storage location**. The package also adds a Replay Control shortcut to desktop application menus.

To remove a standalone install, run `sudo apt remove replay-control` on Debian or `sudo dnf remove replay-control` on Fedora. Your settings and library data remain available for a later reinstall.
