#!/bin/sh
# AppLoad entry point for 贤者笔记 (takeover mode). AppLoad runs this inside
# xochitl's world, which is about to be stopped, so detach the real launch
# into a transient systemd unit (PID-1-owned) and exit immediately.
# ExecStopPost restores xochitl even if the notebook is killed.
HERE=$(cd "$(dirname "$0")" && pwd)
systemctl is-active --quiet sage-takeover && exit 0
systemctl is-active --quiet riddle-takeover && exit 0   # one e-ink app at a time
systemd-run --unit=sage-takeover --collect \
    --property="ExecStopPost=-/bin/sh -c 'systemctl reset-failed xochitl; systemctl start xochitl'" \
    /bin/bash "$HERE/sage-takeover.sh" \
  || systemd-run --unit=sage-takeover --collect /bin/bash "$HERE/sage-takeover.sh"
exit 0
