#!/usr/bin/env python3
"""
Stability test for the replay libretro core.

Loads the .so via ctypes and calls retro_run in multiple cycles to simulate
the RePlayOS frontend's load/run/unload pattern. Tests for heap corruption
and SIGSEGV that would manifest in the dlopen context.

Usage:
    python3 test_stability.py [frames] [cycles]

Default: 36000 frames per cycle, 3 cycles.
"""

import ctypes
import ctypes.util
import os
import signal
import struct
import sys
import threading
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

shutdown_requested = False
last_video_ptr = None  # Track if the video pointer changes unexpectedly

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
    global last_video_ptr
    if data and last_video_ptr and data != last_video_ptr:
        # In a real frontend, this would be a problem if DRM is caching the pointer
        pass
    last_video_ptr = data

def audio_sample_callback(left, right):
    pass

def audio_sample_batch_callback(data, frames):
    return frames

def input_poll_callback():
    pass

# Simulate occasional button presses to exercise navigation code paths
nav_frame = 0
def input_state_callback(port, device, index, id):
    global nav_frame
    # Every 600 frames (~10 seconds), press right to navigate
    if nav_frame > 0 and nav_frame % 600 == 0 and id == 7:  # JOYPAD_RIGHT
        return 1
    # Every 1800 frames (~30 seconds), press Start to toggle list mode
    if nav_frame > 0 and nav_frame % 1800 == 0 and id == 3:  # JOYPAD_START
        return 1
    # Every 900 frames (~15 seconds), press R1 to change page
    if nav_frame > 0 and nav_frame % 900 == 0 and id == 11:  # JOYPAD_R
        return 1
    return 0

# ─── Concurrent reader thread (simulates DRM display thread) ────────────

def concurrent_reader(core, stop_event):
    """Simulate the host's DRM thread reading the last presented frame.

    RePlayOS's display thread may read the framebuffer pointer handed to
    the video callback at any time. This thread exercises that pattern
    to catch data races.
    """
    while not stop_event.is_set():
        if last_video_ptr:
            try:
                # Read first and last pixel from the presented buffer
                buf = (ctypes.c_uint32 * 2).from_address(last_video_ptr)
                _ = buf[0]  # Read first pixel
            except (ValueError, OSError):
                pass  # Buffer may not be mapped
        time.sleep(0.001)  # ~1000 Hz polling

# ─── Main ────────────────────────────────────────────────────────────────

def main():
    global shutdown_requested, nav_frame, last_video_ptr

    total_frames = int(sys.argv[1]) if len(sys.argv) > 1 else 36000
    cycles = int(sys.argv[2]) if len(sys.argv) > 2 else 3

    # Find the .so
    so_path = os.path.join(os.path.dirname(__file__), "target", "release", "libreplay_libretro_core.so")
    if not os.path.exists(so_path):
        print(f"ERROR: {so_path} not found. Run: cargo build --release")
        sys.exit(1)

    print(f"Loading: {so_path}")
    print(f"Target: {total_frames} frames x {cycles} cycles ({total_frames * cycles / 60:.1f}s total at 60fps)")
    print()

    core = ctypes.CDLL(so_path)

    # Set up callbacks (keep references alive!)
    env_cb = ENVIRONMENT_CB(environment_callback)
    video_cb = VIDEO_REFRESH_CB(video_refresh_callback)
    audio_cb = AUDIO_SAMPLE_CB(audio_sample_callback)
    audio_batch_cb = AUDIO_SAMPLE_BATCH_CB(audio_sample_batch_callback)
    input_poll_cb = INPUT_POLL_CB(input_poll_callback)
    input_state_cb = INPUT_STATE_CB(input_state_callback)

    # Get system info
    info = RetroSystemInfo()
    core.retro_get_system_info(ctypes.byref(info))
    print(f"Core: {info.library_name.decode()} v{info.library_version.decode()}")
    print()

    grand_start = time.monotonic()
    total_completed = 0

    for cycle in range(1, cycles + 1):
        print(f"=== Cycle {cycle}/{cycles} ===")
        shutdown_requested = False
        nav_frame = 0
        last_video_ptr = None

        # Initialize
        core.retro_set_environment(env_cb)
        core.retro_set_video_refresh(video_cb)
        core.retro_set_audio_sample(audio_cb)
        core.retro_set_audio_sample_batch(audio_batch_cb)
        core.retro_set_input_poll(input_poll_cb)
        core.retro_set_input_state(input_state_cb)

        core.retro_init()

        # Load game (no-game mode)
        result = core.retro_load_game(None)
        if not result:
            print("ERROR: retro_load_game failed")
            sys.exit(1)

        # Get AV info
        av_info = RetroSystemAvInfo()
        core.retro_get_system_av_info(ctypes.byref(av_info))
        print(f"  Resolution: {av_info.geometry.base_width}x{av_info.geometry.base_height}")

        # Start concurrent reader thread (simulates DRM display thread)
        stop_event = threading.Event()
        reader_thread = threading.Thread(
            target=concurrent_reader, args=(core, stop_event), daemon=True
        )
        reader_thread.start()

        # Run frames
        start = time.monotonic()
        report_interval = 3600
        frame_count = 0

        for i in range(total_frames):
            if shutdown_requested:
                print(f"  Core requested shutdown at frame {i}")
                break

            nav_frame = i
            core.retro_run()
            frame_count += 1

            if frame_count % report_interval == 0:
                elapsed = time.monotonic() - start
                game_secs = frame_count / 60.0
                print(f"  Frame {frame_count:>6} / {total_frames}  "
                      f"({game_secs:.0f}s game time, {elapsed:.1f}s wall time)")

        # Stop concurrent reader
        stop_event.set()
        reader_thread.join(timeout=2.0)

        elapsed = time.monotonic() - start
        total_completed += frame_count
        print(f"  Cycle {cycle} complete: {frame_count} frames in {elapsed:.1f}s")

        # Clean up (mimics the RePlayOS unload/reinit sequence)
        core.retro_unload_game()
        core.retro_deinit()
        print()

    grand_elapsed = time.monotonic() - grand_start
    game_secs = total_completed / 60.0

    print(f"STABLE: {total_completed} frames across {cycles} cycles without crash")
    print(f"  Game time: {game_secs:.1f}s ({game_secs/60:.1f} min)")
    print(f"  Wall time: {grand_elapsed:.1f}s")
    print(f"  Throughput: {total_completed / grand_elapsed:.0f} frames/sec")

if __name__ == "__main__":
    main()
