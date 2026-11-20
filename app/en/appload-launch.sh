#!/bin/sh
# "The Wise's Notes": the same notebook as 贤者笔记 (binary, persona, key and
# memory in ../sage-notes), with the English guide, sleep card and messages.
SAGE=/home/root/xovi/exthome/appload/sage-notes
systemctl is-active --quiet sage-takeover && exit 0
systemctl is-active --quiet riddle-takeover && exit 0   # one e-ink app at a time
systemd-run --unit=sage-takeover --collect \
    --setenv=SAGE_UI_LANG=en --setenv="SAGE_TITLE=The Wise's Notes" \
    --property="ExecStopPost=-/bin/sh -c 'systemctl reset-failed xochitl; systemctl start xochitl'" \
    /bin/bash "$SAGE/sage-takeover.sh" \
  || systemd-run --unit=sage-takeover --collect \
    --setenv=SAGE_UI_LANG=en --setenv="SAGE_TITLE=The Wise's Notes" \
    /bin/bash "$SAGE/sage-takeover.sh"
exit 0
