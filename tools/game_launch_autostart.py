#!/usr/bin/env python3
"""
game_launch_autostart.py - Launch a game on RePlayOS by writing an autostart
file and restarting the replay service.

Launches a game by writing an autostart file and restarting the service.
It works by leveraging the replay binary's built-in autostart mechanism:

  1. Write a .auto file containing the ROM path
  2. Restart the replay service
  3. The binary reads the autostart file on boot and launches the game
  4. Clean up the autostart file after a delay

Usage (run on Pi directly):
    python3 game_launch_autostart.py "/roms/arcade_fbneo/akatana.zip"

Usage (run from host, SSHing to Pi):
    python3 game_launch_autostart.py --ssh root@replay.local "/roms/arcade_fbneo/akatana.zip"
"""

import argparse
import os
import subprocess
import sys
import time

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

STORAGE_PATH = "/media/usb"
AUTOSTART_DIR = "_autostart"
AUTOSTART_FILE = "autostart.auto"
SERVICE_NAME = "replay.service"
DEFAULT_SSH_TARGET = "root@replay.local"
SSH_PASSWORD = "replayos"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def run_cmd(cmd, ssh_target=None, check=True, timeout=30):
    """Run a command locally or via SSH."""
    if ssh_target:
        # Use sshpass for non-interactive password auth
        full_cmd = [
            "sshpass", "-p", SSH_PASSWORD,
            "ssh", "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            "-o", "ConnectTimeout=5",
            ssh_target,
            cmd,
        ]
    else:
        full_cmd = ["sh", "-c", cmd]

    print(f"  $ {cmd}")
    result = subprocess.run(
        full_cmd,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    if result.stdout.strip():
        for line in result.stdout.strip().split("\n"):
            print(f"    {line}")
    if result.stderr.strip():
        for line in result.stderr.strip().split("\n"):
            # Filter out ssh known_hosts warnings
            if "Warning: Permanently added" in line:
                continue
            print(f"    (stderr) {line}")
    if check and result.returncode != 0:
        print(f"  [!] Command failed with exit code {result.returncode}")
        return None
    return result


def write_autostart_file(rom_path, ssh_target=None):
    """Write the autostart file on the Pi."""
    autostart_dir = f"{STORAGE_PATH}/roms/{AUTOSTART_DIR}"
    autostart_path = f"{autostart_dir}/{AUTOSTART_FILE}"

    print(f"\n  [*] Creating autostart directory: {autostart_dir}")
    run_cmd(f"mkdir -p '{autostart_dir}'", ssh_target=ssh_target)

    print(f"  [*] Writing ROM path to {autostart_path}")
    # Escape the rom_path for shell
    escaped = rom_path.replace("'", "'\\''")
    run_cmd(f"echo '{escaped}' > '{autostart_path}'", ssh_target=ssh_target)

    # Verify
    result = run_cmd(f"cat '{autostart_path}'", ssh_target=ssh_target, check=False)
    if result and result.returncode == 0:
        print(f"  [*] Autostart file written successfully")
        return True
    else:
        print(f"  [!] Failed to verify autostart file")
        return False


def restart_service(ssh_target=None):
    """Restart the replay service."""
    print(f"\n  [*] Restarting {SERVICE_NAME}...")
    result = run_cmd(f"systemctl restart {SERVICE_NAME}", ssh_target=ssh_target, check=False)
    if result is None or result.returncode != 0:
        print(f"  [!] Warning: restart command may have failed")
        return False

    # Wait a moment then check status
    print(f"  [*] Waiting 3 seconds for service to start...")
    time.sleep(3)

    result = run_cmd(
        f"systemctl is-active {SERVICE_NAME}",
        ssh_target=ssh_target,
        check=False,
    )
    if result and "active" in result.stdout:
        print(f"  [*] Service is active")
        return True
    else:
        print(f"  [!] Service may not be running")
        return False


def cleanup_autostart(ssh_target=None, delay=5):
    """Remove the autostart file after a delay (the binary should have read it)."""
    autostart_path = f"{STORAGE_PATH}/roms/{AUTOSTART_DIR}/{AUTOSTART_FILE}"

    if delay > 0:
        print(f"\n  [*] Waiting {delay}s before cleanup (letting binary read the file)...")
        time.sleep(delay)

    print(f"  [*] Removing {autostart_path}")
    run_cmd(f"rm -f '{autostart_path}'", ssh_target=ssh_target, check=False)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Launch a game on RePlayOS via autostart file + service restart"
    )
    parser.add_argument(
        "rom_path",
        help='ROM path (e.g., "/roms/arcade_fbneo/akatana.zip" or absolute)'
    )
    parser.add_argument(
        "--ssh",
        metavar="USER@HOST",
        default=None,
        help=f"SSH target (e.g., {DEFAULT_SSH_TARGET}). If omitted, runs locally."
    )
    parser.add_argument(
        "--no-cleanup",
        action="store_true",
        help="Don't remove the autostart file after launching"
    )
    parser.add_argument(
        "--cleanup-delay",
        type=int,
        default=5,
        help="Seconds to wait before removing autostart file (default: 5)"
    )
    parser.add_argument(
        "--no-restart",
        action="store_true",
        help="Only write the autostart file, don't restart the service"
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print what would be done without doing it"
    )

    args = parser.parse_args()

    ssh_target = args.ssh

    print(f"\n{'='*60}")
    print(f"  game_launch_autostart.py")
    print(f"{'='*60}")
    print(f"  ROM path:    {args.rom_path}")
    print(f"  SSH target:  {ssh_target or '(local)'}")
    print(f"  Dry run:     {args.dry_run}")
    print()

    if args.dry_run:
        autostart_dir = f"{STORAGE_PATH}/roms/{AUTOSTART_DIR}"
        autostart_path = f"{autostart_dir}/{AUTOSTART_FILE}"
        print(f"  Would do:")
        print(f"    1. mkdir -p {autostart_dir}")
        print(f"    2. echo '{args.rom_path}' > {autostart_path}")
        if not args.no_restart:
            print(f"    3. systemctl restart {SERVICE_NAME}")
            print(f"    4. Wait 3s, check service status")
        if not args.no_cleanup:
            print(f"    5. Wait {args.cleanup_delay}s, rm -f {autostart_path}")
        print(f"\n  Dry run complete.\n")
        return

    # Check sshpass is available if using SSH
    if ssh_target:
        try:
            subprocess.run(["which", "sshpass"], capture_output=True, check=True)
        except (subprocess.CalledProcessError, FileNotFoundError):
            print("  [!] sshpass not found. Install it:")
            print("      sudo dnf install sshpass   # Fedora")
            print("      sudo apt install sshpass   # Debian/Ubuntu")
            print()
            print("  Alternatively, set up SSH key auth and modify this script.")
            sys.exit(1)

    # Step 1: Write autostart file
    if not write_autostart_file(args.rom_path, ssh_target=ssh_target):
        print("  [!] Failed to write autostart file, aborting.")
        sys.exit(1)

    # Step 2: Restart service
    if not args.no_restart:
        restart_service(ssh_target=ssh_target)
    else:
        print("\n  [*] Skipping service restart (--no-restart)")

    # Step 3: Cleanup
    if not args.no_cleanup:
        cleanup_autostart(ssh_target=ssh_target, delay=args.cleanup_delay)
    else:
        print("\n  [*] Skipping cleanup (--no-cleanup)")

    print(f"\n{'='*60}")
    print(f"  Done!")
    print(f"{'='*60}\n")


if __name__ == "__main__":
    main()
