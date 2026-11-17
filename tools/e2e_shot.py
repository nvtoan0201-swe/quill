#!/usr/bin/env python3
"""End-to-end driver: runs Quill with QUILL_SCRIPT in a throwaway home and
captures the window (X11, `import`) on every `shot:` step.

    tools/e2e_shot.py "wait:1500;;key:f7;;wait:1500;;shot:/tmp/a.png;;action:quill::Quit" FILE…

Run from a desktop session; Quill is started with WAYLAND_DISPLAY unset so it uses X11.
"""
import os, re, subprocess, sys, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
script, args = sys.argv[1], sys.argv[2:]
home = os.environ.get("E2E_HOME", "/tmp/quill-e2e/home")
run = os.path.join(os.path.dirname(home), "run")
os.makedirs(run, mode=0o700, exist_ok=True)
env = dict(os.environ, HOME=home, WAYLAND_DISPLAY="", QUILL_SCRIPT=script,
           XDG_CONFIG_HOME=f"{home}/.config", XDG_DATA_HOME=f"{home}/.local/share", XDG_RUNTIME_DIR=run)
if os.environ.get("E2E_KEEP_HOME"):
    # Keep the real HOME (Claude Code reads its sign-in from there); Quill's own
    # config, data and library still live in the throwaway directory.
    env["HOME"] = os.environ["HOME"]
    import json
    cfg = f"{home}/.config/quill/settings.json"
    os.makedirs(os.path.dirname(cfg), exist_ok=True)
    data = json.load(open(cfg)) if os.path.exists(cfg) else {}
    data["library_dir"] = f"{home}/Documents"
    json.dump(data, open(cfg, "w"))
p = subprocess.Popen([os.path.join(ROOT, "target/debug/quill"), *args], env=env,
                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
deadline = time.time() + 180
for line in p.stdout:
    sys.stdout.write(line)
    m = re.match(r"QUILL_SHOT (\S+)", line)
    if m:
        tree = subprocess.run(["xwininfo", "-root", "-tree"], capture_output=True, text=True).stdout
        wid = next((l.split()[0] for l in tree.splitlines() if "Quill" in l and '("quill"' in l), None)
        if wid:
            subprocess.run(["import", "-window", wid, m.group(1)])
            print("captured", m.group(1))
    if "QUILL_DONE" in line or time.time() > deadline:
        break
p.terminate()
