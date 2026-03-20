#!/usr/bin/env python3
"""
Stability test for the replay libretro core.

Loads the .so via ctypes and calls retro_run 36000 times (10 min at 60fps).
This tests for heap corruption / SIGSEGV that would manifest in the dlopen context.

Usage:
    python3 test_stability.py [frames]

Default: 36000 frames. Pass a number to override.
"""

import ctypes
import ctypes.util
import os
import signal
import struct
import sys
import time

# ─── Libretro constants ─────────────────────────────────────────────────

RETRO_ENVIRONMENT_SET_PIXEL_FORMAT = 10
RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME = 18
RETRO_ENVIRONMENT_SHUTDOWN = 16
RETRO_PIXEL_FORMAT_XRGB8888 = 1
RETRO_DEVICE_JOYPAD = 1

# ─── Libretro structs ───────────────────────────────────────────────────

class RetroSystemInfo(ctypes.Structure):
    _fields_ = [
        ("library_name", ctypes.c_char_p),
        ("library_version", ctypes.c_char_p),
        ("valid_extensions", ctypes.c_char_p),
        ("need_fullpath", ctypes.c_bool),
        ("block_extract", ctypes.c_bool),
    ]

class RetroGameGeometry(ctypes.Structure):
    _fields_ = [
        ("base_width", ctypes.c_uint),
        ("base_height", ctypes.c_uint),
        ("max_width", ctypes.c_uint),
        ("max_height", ctypes.c_uint),
        ("aspect_ratio", ctypes.c_float),
    ]

class RetroSystemTiming(ctypes.Structure):
    _fields_ = [
        ("fps", ctypes.c_double),
        ("sample_rate", ctypes.c_double),
    ]

class RetroSystemAvInfo(ctypes.Structure):
    _fields_ = [
        ("geometry", RetroGameGeometry),
        ("timing", RetroSystemTiming),
    ]

class RetroGameInfo(ctypes.Structure):
    _fields_ = [
        ("path", ctypes.c_char_p),
        ("data", ctypes.c_void_p),
        ("size", ctypes.c_size_t),
        ("meta", ctypes.c_char_p),
    ]

# ─── Callback types ─────────────────────────────────────────────────────

ENVIRONMENT_CB = ctypes.CFUNCTYPE(ctypes.c_bool, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p))
VIDEO_REFRESH_CB = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint, ctypes.c_size_t)
AUDIO_SAMPLE_CB = ctypes.CFUNCTYPE(None, ctypes.c_int16, ctypes.c_int16)
AUDIO_SAMPLE_BATCH_CB = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.POINTER(ctypes.c_int16), ctypes.c_size_t)
INPUT_POLL_CB = ctypes.CFUNCTYPE(None)
INPUT_STATE_CB = ctypes.CFUNCTYPE(ctypes.c_int16, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint)

# ─── Callback implementations ───────────────────────────────────────────

frame_count = 0
shutdown_requested = False

def environment_callback(cmd, data):
    global shutdown_requested
    if cmd == RETRO_ENVIRONMENT_SET_PIXEL_FORMAT:
        return True
    if cmd == RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME:
        return True
    if cmd == RETRO_ENVIRONMENT_SHUTDOWN:
        shutdown_requested = True
        return True
    return False

def video_refresh_callback(data, width, height, pitch):
    global frame_count
    # Just accept the frame, don't do anything with it
    pass

def audio_sample_callback(left, right):
    pass

def audio_sample_batch_callback(data, frames):
    return frames

def input_poll_callback():
    pass

def input_state_callback(port, device, index, id):
    return 0

# ─── Main ────────────────────────────────────────────────────────────────

def main():
    total_frames = int(sys.argv[1]) if len(sys.argv) > 1 else 36000

    # Find the .so
    so_path = os.path.join(os.path.dirname(__file__), "target", "release", "libreplay_libretro_core.so")
    if not os.path.exists(so_path):
        print(f"ERROR: {so_path} not found. Run: cargo build --release")
        sys.exit(1)

    print(f"Loading: {so_path}")
    print(f"Target: {total_frames} frames ({total_frames / 60:.1f}s at 60fps)")
    print()

    core = ctypes.CDLL(so_path)

    # Set up callbacks
    env_cb = ENVIRONMENT_CB(environment_callback)
    video_cb = VIDEO_REFRESH_CB(video_refresh_callback)
    audio_cb = AUDIO_SAMPLE_CB(audio_sample_callback)
    audio_batch_cb = AUDIO_SAMPLE_BATCH_CB(audio_sample_batch_callback)
    input_poll_cb = INPUT_POLL_CB(input_poll_callback)
    input_state_cb = INPUT_STATE_CB(input_state_callback)

    # Initialize
    core.retro_set_environment(env_cb)
    core.retro_set_video_refresh(video_cb)
    core.retro_set_audio_sample(audio_cb)
    core.retro_set_audio_sample_batch(audio_batch_cb)
    core.retro_set_input_poll(input_poll_cb)
    core.retro_set_input_state(input_state_cb)

    core.retro_init()

    # Get system info
    info = RetroSystemInfo()
    core.retro_get_system_info(ctypes.byref(info))
    print(f"Core: {info.library_name.decode()} v{info.library_version.decode()}")

    # Load game (no-game mode)
    result = core.retro_load_game(None)
    print(f"retro_load_game: {'OK' if result else 'FAILED'}")
    if not result:
        print("ERROR: retro_load_game failed")
        sys.exit(1)

    # Get AV info
    av_info = RetroSystemAvInfo()
    core.retro_get_system_av_info(ctypes.byref(av_info))
    print(f"Resolution: {av_info.geometry.base_width}x{av_info.geometry.base_height}")
    print(f"FPS: {av_info.timing.fps}")
    print()

    # Run frames
    start = time.monotonic()
    report_interval = 3600  # report every 60 seconds of game time
    frame_count = 0

    print(f"Running {total_frames} frames...")
    for i in range(total_frames):
        if shutdown_requested:
            print(f"\nCore requested shutdown at frame {i}")
            break

        core.retro_run()
        frame_count += 1

        if frame_count % report_interval == 0:
            elapsed = time.monotonic() - start
            game_secs = frame_count / 60.0
            print(f"  Frame {frame_count:>6} / {total_frames}  "
                  f"({game_secs:.0f}s game time, {elapsed:.1f}s wall time)")

    elapsed = time.monotonic() - start
    game_secs = frame_count / 60.0
    fps = frame_count / elapsed if elapsed > 0 else 0

    print()
    print(f"STABLE: {frame_count} frames completed without crash")
    print(f"  Game time: {game_secs:.1f}s ({game_secs/60:.1f} min)")
    print(f"  Wall time: {elapsed:.1f}s")
    print(f"  Throughput: {fps:.0f} frames/sec")

    # Clean up
    core.retro_unload_game()
    core.retro_deinit()

if __name__ == "__main__":
    main()
