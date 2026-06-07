#!/usr/bin/env python3
"""Probe the official RePlayOS REST API.

This is a standalone diagnostic for validating RePlayOS API behavior before
Replay Control depends on it. The default run is read-only. Actions that alter
frontend state require explicit flags, and dangerous actions require
``--dangerous``.

Examples:
    python3 tools/replayos-api-probe.py --host replay.local --token 123456
    python3 tools/replayos-api-probe.py --replay-cfg /media/sd/config/replay.cfg
    python3 tools/replayos-api-probe.py --host 192.168.10.30 --token 123456 --json-out report.json
    python3 tools/replayos-api-probe.py --host replay.local --token 123456 --safe-actions --set-msg
    python3 tools/replayos-api-probe.py --host replay.local --token 123456 --launch-test --system snes --game-file 'Super Pang (Japan).sfc'
"""

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


DEFAULT_PORT = 55356
DEFAULT_TIMEOUT = 5.0
DEFAULT_MESSAGE = "Replay Control API probe"
HEADER_TOKEN = "X-RePlay-Token"
RETRY_AFTER = "Retry-After"
DEFAULT_REPLAY_CFG = "/media/sd/config/replay.cfg"
REDACTED = "<redacted>"
SENSITIVE_KEYS = {
    "replay_http_token",
    "wifi_pwd",
    "wifi_password",
    "nfs_server",
    "nfs_share",
    "rcheevos_password",
}


class ProbeError(Exception):
    """Raised for local probe configuration errors."""


def parse_replay_cfg_value(content, key):
    prefix = f"{key} "
    for raw_line in content.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or not line.startswith(prefix):
            continue
        if "=" not in line:
            continue
        value = line.split("=", 1)[1].strip()
        if len(value) >= 2 and value[0] == value[-1] == '"':
            value = value[1:-1]
        return value
    return None


def read_token_from_replay_cfg(path):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            content = handle.read()
    except OSError as exc:
        raise ProbeError(f"failed to read replay.cfg at {path}: {exc}") from exc
    token = parse_replay_cfg_value(content, "replay_http_token")
    if not token:
        raise ProbeError(f"replay_http_token is missing or empty in {path}")
    return token


def now_ms():
    return int(time.time() * 1000)


def compact_json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def is_success(status):
    return 200 <= status < 300


def parse_json_or_none(text):
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def redact_json(value):
    if isinstance(value, dict):
        redacted = {}
        for key, item in value.items():
            if str(key) in SENSITIVE_KEYS:
                redacted[key] = REDACTED
            else:
                redacted[key] = redact_json(item)
        return redacted
    if isinstance(value, list):
        return [redact_json(item) for item in value]
    return value


def redact_params(params):
    redacted = dict(params)
    option = redacted.get("option")
    if option in SENSITIVE_KEYS and "value" in redacted:
        redacted["value"] = REDACTED
    for key in list(redacted):
        if key in SENSITIVE_KEYS:
            redacted[key] = REDACTED
    return redacted


def report_body(body):
    parsed = parse_json_or_none(body)
    if parsed is None:
        return body
    return compact_json(redact_json(parsed))


def selected_headers(headers):
    keep = {}
    for name in [RETRY_AFTER, "Content-Type", "Content-Length"]:
        value = headers.get(name)
        if value is not None:
            keep[name] = value
    return keep


class RePlayApiClient:
    def __init__(self, base_url, token, timeout):
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.timeout = timeout

    def get(self, endpoint, params=None, name=None):
        params = params or {}
        query = urllib.parse.urlencode(params)
        redacted_params = redact_params(params)
        redacted_query = urllib.parse.urlencode(redacted_params)
        url = f"{self.base_url}/{endpoint.lstrip('/')}"
        if query:
            request_url = f"{url}?{query}"
            display_url = f"{url}?{redacted_query}"
        else:
            request_url = url
            display_url = url

        started = now_ms()
        req = urllib.request.Request(request_url, method="GET")
        req.add_header(HEADER_TOKEN, self.token)
        req.add_header("Accept", "application/json")

        result = {
            "name": name or endpoint,
            "endpoint": endpoint,
            "params": redacted_params,
            "url": display_url,
            "status": None,
            "headers": {},
            "duration_ms": None,
            "body": "",
            "json": None,
            "error": None,
        }

        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                body = resp.read().decode("utf-8", errors="replace")
                result["status"] = resp.status
                result["headers"] = selected_headers(resp.headers)
                parsed = parse_json_or_none(body)
                result["body"] = report_body(body)
                result["json"] = redact_json(parsed) if parsed is not None else None
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", errors="replace")
            parsed = parse_json_or_none(body)
            result["status"] = exc.code
            result["headers"] = selected_headers(exc.headers)
            result["body"] = report_body(body)
            result["json"] = redact_json(parsed) if parsed is not None else None
            result["error"] = f"HTTP {exc.code}"
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            result["error"] = str(exc)
        finally:
            result["duration_ms"] = now_ms() - started

        return result


class ProbeReport:
    def __init__(self, base_url):
        self.base_url = base_url
        self.started_at_unix = int(time.time())
        self.requests = []
        self.checks = []
        self.inferences = []

    def add_request(self, result):
        self.requests.append(result)
        return result

    def add_check(self, name, ok, detail):
        self.checks.append({"name": name, "ok": bool(ok), "detail": detail})

    def add_inference(self, name, value, detail=None):
        item = {"name": name, "value": value}
        if detail is not None:
            item["detail"] = detail
        self.inferences.append(item)

    def ok(self):
        return all(check["ok"] for check in self.checks)

    def as_json(self):
        return {
            "base_url": self.base_url,
            "started_at_unix": self.started_at_unix,
            "requests": self.requests,
            "checks": self.checks,
            "inferences": self.inferences,
            "ok": self.ok(),
        }


def result_summary(result):
    status = result["status"] if result["status"] is not None else "-"
    error = result["error"] or ""
    if result["json"] is not None:
        body = compact_json(result["json"])
    else:
        body = result["body"].replace("\n", "\\n")
    if len(body) > 110:
        body = body[:107] + "..."
    return status, f"{result['duration_ms']}ms", error, body


def print_section(title):
    print()
    print(title)
    print("-" * len(title))


def print_request(result, verbose=False):
    status, duration, error, body = result_summary(result)
    print(f"{result['name']:<24} status={status:<4} duration={duration:<8} {error}")
    if verbose:
        print(f"  url: {result['url']}")
        if result["headers"]:
            print(f"  headers: {compact_json(result['headers'])}")
        if body:
            print(f"  body: {body}")


def print_report(report, verbose=False):
    print_section("Requests")
    for result in report.requests:
        print_request(result, verbose=verbose)

    print_section("Checks")
    for check in report.checks:
        mark = "OK" if check["ok"] else "FAIL"
        print(f"{mark:<4} {check['name']}: {check['detail']}")

    if report.inferences:
        print_section("Inferences")
        for item in report.inferences:
            detail = item.get("detail")
            suffix = f" ({detail})" if detail else ""
            print(f"{item['name']}: {item['value']}{suffix}")


def write_json_report(report, path):
    data = json.dumps(report.as_json(), indent=2, sort_keys=True)
    if path == "-":
        print(data)
        return
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(data)
        handle.write("\n")


def require_success(report, result, check_name):
    ok = result["status"] is not None and is_success(result["status"])
    detail = f"status={result['status']}"
    if result["error"]:
        detail = f"{detail} error={result['error']}"
    report.add_check(check_name, ok, detail)
    return ok


def infer_auth_behavior(report, result):
    if result["status"] == 401:
        report.add_inference("auth", "unauthorized", "token was rejected")
    elif result["status"] == 429:
        retry = result["headers"].get(RETRY_AFTER)
        detail = f"retry-after={retry}" if retry else "no retry-after header"
        report.add_inference("auth", "rate_limited", detail)


def status_game_file(status_json):
    if not isinstance(status_json, dict):
        return None
    value = status_json.get("game_file")
    if isinstance(value, str) and value:
        return value
    return None


def status_view(status_json):
    if not isinstance(status_json, dict):
        return None, None
    return status_json.get("view"), status_json.get("view_id")


def classify_status(report, label, result):
    data = result["json"]
    if not isinstance(data, dict):
        return
    view, view_id = status_view(data)
    game_file = status_game_file(data)
    paused = data.get("paused")
    if game_file:
        if paused:
            state = "playing_paused"
        elif view_id == 2 or view == "game_play":
            state = "playing"
        else:
            state = "playing_overlay_or_menu"
    elif view_id in (0, 1) or view in ("system_list", "system_options"):
        state = "menu"
    else:
        state = "unknown"
    report.add_inference(
        f"{label}_status",
        state,
        f"view={view!r} view_id={view_id!r} paused={paused!r} game_file={game_file!r}",
    )


def run_read_only(client, report, status_samples, status_interval):
    version = report.add_request(client.get("get_version", name="get_version"))
    require_success(report, version, "get_version")
    infer_auth_behavior(report, version)

    status = report.add_request(client.get("get_status", name="get_status"))
    if require_success(report, status, "get_status"):
        classify_status(report, "initial", status)
    infer_auth_behavior(report, status)

    for sample in range(2, status_samples + 1):
        if status_interval > 0:
            time.sleep(status_interval)
        sampled = report.add_request(
            client.get("get_status", name=f"get_status_sample_{sample}")
        )
        if require_success(report, sampled, f"get_status_sample_{sample}"):
            classify_status(report, f"sample_{sample}", sampled)
        infer_auth_behavior(report, sampled)

    config = report.add_request(client.get("get_replay_config", name="get_replay_config"))
    require_success(report, config, "get_replay_config")
    infer_auth_behavior(report, config)

    media = report.add_request(client.get("get_media_status", name="get_media_status"))
    if require_success(report, media, "get_media_status"):
        data = media["json"]
        if isinstance(data, dict):
            available = data.get("available")
            count = data.get("count")
            current = data.get("current_index")
            report.add_inference(
                "media_status",
                "available" if available else "unavailable",
                f"count={count!r} current_index={current!r}",
            )
    infer_auth_behavior(report, media)


def run_set_msg(client, report, text, duration):
    result = report.add_request(
        client.get(
            "set_msg",
            {"text": text, "duration": str(duration)},
            name="set_msg",
        )
    )
    require_success(report, result, "set_msg")


def run_set_cmd(client, report, cmd, name=None):
    result = report.add_request(client.get("set_cmd", {"cmd": cmd}, name=name or f"set_cmd:{cmd}"))
    require_success(report, result, f"set_cmd:{cmd}")
    return result


def wait_and_status(client, report, label, delay):
    if delay > 0:
        time.sleep(delay)
    result = report.add_request(client.get("get_status", name=label))
    if require_success(report, result, label):
        classify_status(report, label, result)
    return result


def run_safe_actions(client, report, args):
    if args.set_msg:
        run_set_msg(client, report, args.message, args.message_duration)
    if args.screenshot:
        run_set_cmd(client, report, "screenshot", name="screenshot")
    for _ in range(args.volume_down):
        run_set_cmd(client, report, "volume_down", name="volume_down")
    for _ in range(args.volume_up):
        run_set_cmd(client, report, "volume_up", name="volume_up")
    if args.mute:
        run_set_cmd(client, report, "mute", name="mute")
    for _ in range(args.mute_toggles):
        run_set_cmd(client, report, "mute", name="mute")


def run_game_reset(client, report, args):
    before = wait_and_status(client, report, "before_game_reset_status", 0)
    result = run_set_cmd(client, report, "game_reset", name="game_reset")
    after = wait_and_status(client, report, "after_game_reset_status", args.post_action_delay)
    if is_success(result["status"] or 0):
        before_file = status_game_file(before["json"])
        after_file = status_game_file(after["json"])
        report.add_check(
            "game_reset_keeps_game_loaded",
            before_file is not None and after_file == before_file,
            f"before={before_file!r} after={after_file!r}",
        )


def run_launch_test(client, report, args):
    before = wait_and_status(client, report, "before_launch_status", 0)
    started = now_ms()
    result = report.add_request(
        client.get(
            "load_game",
            {"system": args.system, "game_file": args.game_file},
            name="load_game",
        )
    )
    ok = require_success(report, result, "load_game")
    after = wait_and_status(client, report, "after_launch_status", args.post_action_delay)
    if ok:
        before_file = status_game_file(before["json"])
        after_file = status_game_file(after["json"])
        changed = after_file != before_file
        contains_target = after_file is not None and after_file.endswith(args.game_file)
        report.add_check(
            "load_game_changed_status",
            changed or contains_target,
            f"before={before_file!r} after={after_file!r}",
        )
        report.add_inference(
            "load_game_api_reachable_after",
            after["status"] is not None,
            f"elapsed_ms={now_ms() - started}",
        )


def run_state_test(client, report, args):
    slot = str(args.slot)
    save = report.add_request(client.get("save_state", {"slot": slot}, name="save_state"))
    require_success(report, save, "save_state")
    if args.load_state:
        load = report.add_request(client.get("load_state", {"slot": slot}, name="load_state"))
        require_success(report, load, "load_state")
        wait_and_status(client, report, "after_load_state_status", args.post_action_delay)


def media_command(client, report, cmd, index=None):
    params = {"cmd": cmd}
    if index is not None:
        params["index"] = str(index)
    result = report.add_request(client.get("set_media", params, name=f"set_media:{cmd}"))
    ok = result["status"] is not None and is_success(result["status"])
    boundary = result["status"] == 409
    report.add_check(
        f"set_media:{cmd}",
        ok or boundary,
        f"status={result['status']} body={result['body'][:120]!r}",
    )
    if boundary:
        report.add_inference(f"set_media:{cmd}", "boundary_or_unavailable", result["body"][:120])
    return result


def run_media_test(client, report, args):
    before = report.add_request(client.get("get_media_status", name="before_media_status"))
    if not require_success(report, before, "before_media_status"):
        return
    data = before["json"]
    if not isinstance(data, dict) or not data.get("available"):
        report.add_check("media_available_for_test", True, "media control is unavailable; skipped")
        report.add_inference("media_test", "skipped", "media control unavailable")
        return
    report.add_check("media_available_for_test", True, f"count={data.get('count')!r}")
    media_command(client, report, "open_tray")
    media_command(client, report, "close_tray")
    if args.media_next:
        media_command(client, report, "next")
    if args.media_previous:
        media_command(client, report, "previous")
    if args.media_index is not None:
        media_command(client, report, "set_index", index=args.media_index)
    after = report.add_request(client.get("get_media_status", name="after_media_status"))
    require_success(report, after, "after_media_status")


def run_config_test(client, report, args):
    before = report.add_request(client.get("get_replay_config", name="before_config"))
    require_success(report, before, "before_config")
    params = {"option": args.config_option, "value": args.config_value}
    result = report.add_request(client.get("set_replay_config", params, name="set_replay_config"))
    ok = require_success(report, result, "set_replay_config")
    after = report.add_request(client.get("get_replay_config", name="after_config"))
    require_success(report, after, "after_config")
    if ok:
        observed = config_json_value(after, args.config_option)
        report.add_check(
            "set_replay_config_observed",
            observed == args.config_value,
            f"{args.config_option} observed={observed!r} expected={args.config_value!r}",
        )


def read_cfg_file_value(path, option):
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return parse_replay_cfg_value(handle.read(), option)
    except OSError as exc:
        raise ProbeError(f"failed to read {path}: {exc}") from exc


def poll_file_for_value(path, option, expected, timeout_secs):
    """Poll replay.cfg until `option` equals `expected`. Returns latency ms or None."""
    started = now_ms()
    deadline = started + int(timeout_secs * 1000)
    while True:
        if read_cfg_file_value(path, option) == expected:
            return now_ms() - started
        if now_ms() >= deadline:
            return None
        time.sleep(0.2)


def service_enter_timestamp():
    """ActiveEnterTimestamp of replay.service, or None off-device."""
    try:
        out = subprocess.run(
            ["systemctl", "show", "replay.service", "-p", "ActiveEnterTimestamp"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    value = out.stdout.strip().removeprefix("ActiveEnterTimestamp=")
    return value or None


def wlan_link_state():
    try:
        out = subprocess.run(
            ["ip", "-br", "link", "show", "wlan0"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    return out.stdout.strip() or None


def config_json_value(result, option):
    """Read a value from a get_replay_config response.

    The response nests values under a `config` object:
    {"modification_num": N, "config": {...}}. `modification_num` itself is
    top-level.
    """
    if not isinstance(result["json"], dict):
        return None
    if option == "modification_num":
        return result["json"].get("modification_num")
    nested = result["json"].get("config")
    if isinstance(nested, dict):
        return nested.get(option)
    return None


def parse_cfg_file_pairs(path):
    """All key=value pairs from replay.cfg, for collateral-change detection."""
    pairs = {}
    try:
        with open(path, "r", encoding="utf-8") as handle:
            content = handle.read()
    except OSError as exc:
        raise ProbeError(f"failed to read {path}: {exc}") from exc
    for raw_line in content.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if len(value) >= 2 and value[0] == value[-1] == '"':
            value = value[1:-1]
        pairs[key] = value
    return pairs


def cfg_pairs_diff(before, after, ignore_key):
    """Keys (other than ignore_key) whose values differ between two snapshots."""
    changes = {}
    for key in sorted(set(before) | set(after)):
        if key == ignore_key:
            continue
        old, new = before.get(key), after.get(key)
        if old != new:
            redact = key in SENSITIVE_KEYS
            changes[key] = {
                "before": REDACTED if redact else old,
                "after": REDACTED if redact else new,
            }
    return changes


def run_config_roundtrip(client, report, args):
    """set_replay_config roundtrip: temp value -> file persistence + restart
    behavior + API observation -> restore original. Answers whether API config
    writes land in replay.cfg (and how fast), whether the frontend restarts
    itself, and whether modification_num tracks changes."""
    option = args.roundtrip_option
    temp_value = args.roundtrip_value
    cfg_path = args.replay_cfg or DEFAULT_REPLAY_CFG
    sensitive = option in SENSITIVE_KEYS
    wifi_key = option.startswith("wifi_")

    original = read_cfg_file_value(cfg_path, option)
    if original is None:
        raise ProbeError(f"{option} not present in {cfg_path}; refusing roundtrip (cannot restore)")
    if original == temp_value:
        raise ProbeError(f"--roundtrip-value equals the current {option}; pick a different value")

    service_before = service_enter_timestamp()
    link_before = wlan_link_state() if wifi_key else None
    file_pairs_before = parse_cfg_file_pairs(cfg_path)

    before = report.add_request(client.get("get_replay_config", name="roundtrip_before_config"))
    require_success(report, before, "roundtrip_before_config")
    mod_before = config_json_value(before, "modification_num")
    api_before = config_json_value(before, option)
    report.add_inference(
        "roundtrip_api_exposes_option_before",
        api_before is not None,
        f"{option} present in get_replay_config: {api_before is not None}",
    )

    set_result = report.add_request(
        client.get("set_replay_config", {"option": option, "value": temp_value}, name="roundtrip_set")
    )
    if not require_success(report, set_result, "roundtrip_set"):
        return

    latency_ms = poll_file_for_value(cfg_path, option, temp_value, timeout_secs=10.0)
    report.add_check(
        "roundtrip_file_persisted",
        latency_ms is not None,
        f"{option} reached {cfg_path} after {latency_ms}ms"
        if latency_ms is not None
        else f"{option} did not reach {cfg_path} within 10s",
    )

    # Surgical-edit vs full-serialize: did the write touch any OTHER key?
    collateral = cfg_pairs_diff(file_pairs_before, parse_cfg_file_pairs(cfg_path), ignore_key=option)
    report.add_check(
        "roundtrip_no_collateral_changes",
        not collateral,
        "no other keys changed" if not collateral else f"other keys changed: {compact_json(collateral)}",
    )

    after = report.add_request(client.get("get_replay_config", name="roundtrip_after_config"))
    require_success(report, after, "roundtrip_after_config")
    mod_after = config_json_value(after, "modification_num")
    report.add_inference("roundtrip_modification_num", f"{mod_before} -> {mod_after}")
    api_after = config_json_value(after, option)
    if sensitive:
        report.add_inference("roundtrip_api_observed", REDACTED, "masked key, compared file-side only")
    else:
        report.add_check(
            "roundtrip_api_observed",
            api_after == temp_value,
            f"{option} api observed={api_after!r} expected={temp_value!r}",
        )

    alive = report.add_request(client.get("get_version", name="roundtrip_api_alive"))
    require_success(report, alive, "roundtrip_api_alive")

    service_after = service_enter_timestamp()
    report.add_check(
        "roundtrip_no_service_restart",
        service_before is not None and service_before == service_after,
        f"ActiveEnterTimestamp {service_before!r} -> {service_after!r}",
    )
    if wifi_key:
        report.add_inference("roundtrip_wlan_state", f"{link_before} -> {wlan_link_state()}")

    restore = report.add_request(
        client.get("set_replay_config", {"option": option, "value": original}, name="roundtrip_restore")
    )
    require_success(report, restore, "roundtrip_restore")
    restore_ms = poll_file_for_value(cfg_path, option, original, timeout_secs=10.0)
    report.add_check(
        "roundtrip_restored",
        restore_ms is not None,
        f"{option} restored in file after {restore_ms}ms"
        if restore_ms is not None
        else f"{option} NOT restored within 10s -- MANUAL RESTORE NEEDED",
    )


def run_power_command(client, report, args):
    if args.reboot:
        run_set_cmd(client, report, "reboot", name="reboot")
    if args.power_off:
        run_set_cmd(client, report, "power_off", name="power_off")


def validate_args(args):
    if args.base_url:
        args.base_url = args.base_url.rstrip("/")
    else:
        args.base_url = f"http://{args.host}:{args.port}/api/v1"

    if not args.token and args.replay_cfg:
        args.token = read_token_from_replay_cfg(args.replay_cfg)
    if not args.token and args.auto_replay_cfg and os.path.exists(DEFAULT_REPLAY_CFG):
        args.token = read_token_from_replay_cfg(DEFAULT_REPLAY_CFG)
    if not args.token:
        args.token = os.environ.get("REPLAYOS_API_TOKEN", "")
    if not args.token:
        raise ProbeError(
            "missing token: pass --token, set REPLAYOS_API_TOKEN, "
            "or use --replay-cfg"
        )

    safe_requested = any([
        args.set_msg,
        args.screenshot,
        args.volume_up,
        args.volume_down,
        args.mute,
        args.mute_toggles,
    ])
    if safe_requested and not args.safe_actions:
        raise ProbeError("safe actions require --safe-actions")

    if args.launch_test and (not args.system or not args.game_file):
        raise ProbeError("--launch-test requires --system and --game-file")

    if args.status_samples < 1:
        raise ProbeError("--status-samples must be at least 1")
    if args.status_interval < 0:
        raise ProbeError("--status-interval must be non-negative")
    if args.mute_toggles < 0:
        raise ProbeError("--mute-toggles must be non-negative")

    if args.state_test and args.slot is None:
        raise ProbeError("--state-test requires --slot")
    if args.slot is not None and not (1 <= args.slot <= 18):
        raise ProbeError("--slot must be between 1 and 18")
    if args.load_state and not args.state_test:
        raise ProbeError("--load-state requires --state-test")

    if args.config_test and (not args.config_option or args.config_value is None):
        raise ProbeError("--config-test requires --config-option and --config-value")
    if args.config_roundtrip:
        if not args.roundtrip_option or args.roundtrip_value is None:
            raise ProbeError("--config-roundtrip requires --roundtrip-option and --roundtrip-value")
        cfg_path = args.replay_cfg or DEFAULT_REPLAY_CFG
        if not os.path.exists(cfg_path):
            raise ProbeError(
                f"--config-roundtrip needs local replay.cfg access ({cfg_path} not found); run on the device"
            )
    if (args.media_next or args.media_previous or args.media_index is not None) and not args.media_test:
        raise ProbeError("--media-next/--media-previous/--media-index require --media-test")

    dangerous_requested = any([
        args.config_test,
        args.config_roundtrip,
        args.reboot,
        args.power_off,
        args.game_reset,
    ])
    if dangerous_requested and not args.dangerous:
        raise ProbeError("config/reboot/power tests require --dangerous")

    if args.power_off and not args.confirm_power_off:
        raise ProbeError("--power-off requires --confirm-power-off")

    if args.reboot and not args.confirm_reboot:
        raise ProbeError("--reboot requires --confirm-reboot")


def build_parser():
    parser = argparse.ArgumentParser(
        description="Probe the official RePlayOS REST API",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--host", default="127.0.0.1", help="RePlayOS host/IP")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help="RePlayOS API port")
    parser.add_argument("--base-url", help="Full API base URL, overrides --host/--port")
    parser.add_argument("--token", help="Net Control code. Defaults to REPLAYOS_API_TOKEN")
    parser.add_argument("--replay-cfg", help="Read replay_http_token from this replay.cfg")
    parser.add_argument(
        "--no-auto-replay-cfg",
        dest="auto_replay_cfg",
        action="store_false",
        default=True,
        help=f"Do not auto-read {DEFAULT_REPLAY_CFG} when present",
    )
    parser.add_argument("--timeout", type=float, default=DEFAULT_TIMEOUT, help="Request timeout seconds")
    parser.add_argument("--json-out", help="Write JSON report to this path, or '-' for stdout")
    parser.add_argument("--verbose", action="store_true", help="Print request URLs, headers, and bodies")
    parser.add_argument("--status-samples", type=int, default=1, help="Number of get_status samples to collect")
    parser.add_argument("--status-interval", type=float, default=1.0, help="Seconds between get_status samples")
    parser.add_argument(
        "--post-action-delay",
        type=float,
        default=2.0,
        help="Seconds to wait before after-action status checks",
    )

    safe = parser.add_argument_group("safe actions")
    safe.add_argument("--safe-actions", action="store_true", help="Allow safe command probes")
    safe.add_argument("--set-msg", action="store_true", help="Display a RePlayOS popup message")
    safe.add_argument("--message", default=DEFAULT_MESSAGE, help="Message text for --set-msg")
    safe.add_argument("--message-duration", type=int, default=3, help="Message duration seconds")
    safe.add_argument("--screenshot", action="store_true", help="Request a screenshot")
    safe.add_argument("--volume-up", type=int, default=0, help="Run volume_up N times")
    safe.add_argument("--volume-down", type=int, default=0, help="Run volume_down N times")
    safe.add_argument("--mute", action="store_true", help="Toggle mute once")
    safe.add_argument("--mute-toggles", type=int, default=0, help="Toggle mute N additional times")

    launch = parser.add_argument_group("launch test")
    launch.add_argument("--launch-test", action="store_true", help="Run load_game")
    launch.add_argument("--system", help="System folder/name for --launch-test")
    launch.add_argument("--game-file", help="ROM path relative to the system folder for --launch-test")

    dangerous = parser.add_argument_group("dangerous or state-changing tests")
    dangerous.add_argument("--dangerous", action="store_true", help="Allow dangerous probes")
    dangerous.add_argument("--state-test", action="store_true", help="Run save_state")
    dangerous.add_argument("--slot", type=int, help="Save/load state slot, 1-18")
    dangerous.add_argument("--load-state", action="store_true", help="Also run load_state after save_state")
    dangerous.add_argument("--media-test", action="store_true", help="Run media control probes when available")
    dangerous.add_argument("--media-next", action="store_true", help="Run set_media next during --media-test")
    dangerous.add_argument("--media-previous", action="store_true", help="Run set_media previous during --media-test")
    dangerous.add_argument("--media-index", type=int, help="Run set_media set_index during --media-test")
    dangerous.add_argument("--game-reset", action="store_true", help="Run set_cmd?cmd=game_reset")
    dangerous.add_argument("--config-test", action="store_true", help="Run set_replay_config")
    dangerous.add_argument("--config-option", help="Config option for --config-test")
    dangerous.add_argument("--config-value", help="Config value for --config-test")
    dangerous.add_argument(
        "--config-roundtrip",
        action="store_true",
        help="set_replay_config roundtrip: set temp value, verify replay.cfg "
        "persistence + frontend restart behavior, restore original (needs local replay.cfg access)",
    )
    dangerous.add_argument("--roundtrip-option", help="Config option for --config-roundtrip")
    dangerous.add_argument("--roundtrip-value", help="Temporary value for --config-roundtrip")
    dangerous.add_argument("--reboot", action="store_true", help="Run set_cmd?cmd=reboot")
    dangerous.add_argument("--confirm-reboot", action="store_true", help="Required with --reboot")
    dangerous.add_argument("--power-off", action="store_true", help="Run set_cmd?cmd=power_off")
    dangerous.add_argument("--confirm-power-off", action="store_true", help="Required with --power-off")

    return parser


def main():
    parser = build_parser()
    args = parser.parse_args()
    try:
        validate_args(args)
    except ProbeError as exc:
        parser.error(str(exc))

    print(f"RePlayOS API probe: {args.base_url}")
    print("Default read-only checks always run.")

    client = RePlayApiClient(args.base_url, args.token, args.timeout)
    report = ProbeReport(args.base_url)

    run_read_only(client, report, args.status_samples, args.status_interval)

    if args.launch_test:
        run_launch_test(client, report, args)
    if args.safe_actions:
        run_safe_actions(client, report, args)
    if args.state_test:
        run_state_test(client, report, args)
    if args.game_reset:
        run_game_reset(client, report, args)
    if args.media_test:
        run_media_test(client, report, args)
    if args.config_test:
        run_config_test(client, report, args)
    if args.config_roundtrip:
        run_config_roundtrip(client, report, args)
    if args.reboot or args.power_off:
        run_power_command(client, report, args)

    print_report(report, verbose=args.verbose)
    if args.json_out:
        write_json_report(report, args.json_out)
        if args.json_out != "-":
            print(f"\nJSON report written to {args.json_out}")

    return 0 if report.ok() else 1


if __name__ == "__main__":
    sys.exit(main())
