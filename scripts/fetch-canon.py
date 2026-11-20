#!/usr/bin/env python3
"""Build app/canon.txt — Hroptatyr's own lines and notebook entries, zh + en.

The corpus is not shipped with the repository: it is HoYoverse's script text.
This script fetches the two quest pages this project's canon points at, lifts
only the lines Hroptatyr speaks (plus the lines spoken *to* him, which the
notebook's easter egg needs as prompts), and writes them to the device-local
canon file that `deploy.sh` never overwrites.

    ./scripts/fetch-canon.py              # writes app/canon.txt if absent
    ./scripts/fetch-canon.py --force      # overwrite (discards your edits)

Sources (see app/canon.example.txt):
    中文  https://wiki.biligame.com/ys/被镌刻的时光   任务剧情
    EN    https://genshin-impact.fandom.com/wiki/Etched_in_Time   Dialogue

Format is the one `src/canon.rs` parses:

    [场景：…]                 (or [Scene: …])
    海洛塔帝：exact line      (or  Hroptatyr: exact line)

A line with no speaker label continues the previous entry (a wrapped line).
"""
import json
import os
import re
import sys
import urllib.parse
import urllib.request

ZPAGE = "被镌刻的时光"
EPAGE = "Etched_in_Time"
ZH_API = "https://wiki.biligame.com/ys/api.php?action=parse&prop=wikitext&format=json&formatversion=2&page="
EN_API = ("https://genshin-impact.fandom.com/api.php?action=parse&prop=wikitext"
          "&format=json&formatversion=2&page=")
UA = "sage-notes-canon/1.0 (unofficial fan project; personal use only)"

HIS_ZH = "海洛塔帝"
HIS_EN = "Hroptatyr"
# Who may appear: his own lines, and the ones written *at* him (the easter egg
# answers these with his next line).
ZH_SPEAKERS = ("海洛塔帝", "艾弗提尔维特", "旅行者", "密麦梅斯")
EN_SPEAKERS = ("Hroptatyr", "Eftirvit", "Traveler", "Mimameith")

# Scene labels for the English page, keyed by its own stage directions.
EN_SCENES = {
    "(A younger Pierro &mdash; Eftirvit &mdash; and Hroptatyr meet in a courtyard in Khaenri'ah)":
        "Bygone Days — the courtyard (PV)",
    "(After a timeskip, the scene shifts to a darker location)":
        "Bygone Days — the professor's decline",
    "(After a timeskip, the scene changes to the professor's hospital room)":
        "Bygone Days — the professor's room",
    "(After a timeskip, Eftirvit finds Hroptatyr holding an umbrella in front of Mimameith's tombstone)":
        "Bygone Days — the gravestone in the rain",
    "(Approach Hroptatyr)": "the seminar under the tree",
    "(Pierro approaches the notebook on the pedestal)": "The Wise's notebook",
    "(Pierro hesitates a few moments more, then picks up the notebook and brushes the cobwebs off before opening it)":
        "The Wise's notebook",
    "(Pierro pulls out what looks like a photograph of himself and Hroptatyr)":
        "The Wise's notebook",
}
# The zh page swaps these two speakers inside the PV block; the other language
# has it right, so fix it rather than ship the slip.
ZH_FIXES = {("海洛塔帝", "呵呵，没什么可惜的"): "密麦梅斯"}


def fetch(api, page):
    url = api + urllib.parse.quote(page)
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=45) as r:
        return json.loads(r.read().decode("utf-8"))["parse"]["wikitext"]


ENTITIES = {
    "&mdash;": "—", "&ndash;": "–", "&nbsp;": " ", "&amp;": "&",
    "&quot;": '"', "&lt;": "<", "&gt;": ">", "&#39;": "'", "&hellip;": "…",
}


def plain(s):
    """Strip the wiki markup that survives inside a spoken line."""
    for k, v in ENTITIES.items():
        s = s.replace(k, v)
    s = re.sub(r"<!--.*?-->", "", s)
    s = re.sub(r"\{\{sic\|[^}]*\}\}", "", s)
    s = re.sub(r"\{\{MC\|m=([^|}]*)\|f=([^|}]*)\}\}", r"\1/\2", s)
    s = re.sub(r"\{\{Traveler\}\}", "Traveler", s)
    s = re.sub(r"\[\[[^\]|]*\|([^\]]*)\]\]", r"\1", s)   # [[page|label]] → label
    s = re.sub(r"\[\[([^\]]*)\]\]", r"\1", s)            # [[page]] → page
    s = re.sub(r"''+(.*?)''+", r"\1", s)                 # italics
    s = re.sub(r"<[^>]+>", "", s)
    s = re.sub(r"\s+", " ", s)
    s = s.replace("}}", "").replace("{{", "")
    return s.strip()


def unwrap_quote(s):
    """`「...」` / `"..."` around a whole line is the wiki's way of showing a
    written passage; the notebook itself holds the text without them."""
    for a, b in (("「", "」"), ("『", "』"), ('"', '"'), ("“", "”")):
        if len(s) > 2 and s.startswith(a) and s.endswith(b) and b not in s[len(a):-len(b)]:
            return s[len(a):-len(b)].strip()
    return s


def zh_lines(wiki):
    """(scene, speaker, text) from the zh 任务剧情 section, in order."""
    start = wiki.find("==任务剧情==")
    body = wiki[start:]
    out, section, in_pv, pv_block = [], "", False, 0
    for raw in body.split("\n"):
        m = re.match(r"^===(.*?)===\s*$", raw)
        if m:
            section = plain(m.group(1))
            in_pv = False
            continue
        if "剧情PV" in raw:
            in_pv = True
            continue
        if in_pv and raw.strip() == "<hr>":
            pv_block += 1       # the PV timeskips between these blocks
            continue
        m = re.match(r"^\*([^：:]{1,12})[：:](.*)$", raw)
        if not m:
            continue
        text = plain(m.group(2))
        # The folding box that holds the PV closes with "}}"; that closes the
        # flashback too, so the quest's present-day dialogue stays out.
        keep = in_pv
        if in_pv and raw.rstrip().endswith("}}"):
            in_pv = False
        # Only the three places this canon is about: the PV flashback, the
        # notebook, and the engraved memory. The rest of the quest is Pierro's
        # present, which the notebook has no part in.
        note = "笔记" in section
        memory = "镌刻" in section
        if not (keep or note or memory):
            continue
        who = m.group(1).strip().strip("「」")
        if who not in ZH_SPEAKERS:
            # 「丑角」 reading the notebook aloud: keep whole-line quotations.
            if who == "丑角" and text.startswith("「") and text.endswith("」"):
                who = HIS_ZH
                text = unwrap_quote(text)
                if text.startswith("「"):
                    continue      # his aside over the page, not an entry
            else:
                continue
        who = ZH_FIXES.get((who, text), who)
        if not text:
            continue
        out.append((zh_scene(note, memory, pv_block), who, text))
    return out


# The PV's four blocks, in order: the courtyard, the professor's decline, her
# room, and the gravestone in the rain.
PV_SCENES = ("往日 · 树下的庭院（PV）", "往日 · 教授的病况", "往日 · 老师的病房", "往日 · 雨中的墓碑")


def zh_scene(note, memory, pv_block):
    if note:
        return "「贤者」的笔记"
    if memory:
        # The engraved memory holds both meetings: Eftirvit's first, then the
        # Traveler's. One label for the place, not for either visitor.
        return "「被镌刻的记忆」"
    return PV_SCENES[min(pv_block, len(PV_SCENES) - 1)]


def en_lines(wiki):
    start = wiki.find("==Dialogue==")
    end = wiki.find("==Gallery==")
    body = wiki[start:end if end > 0 else len(wiki)]
    out, scene = [], "Etched in Time"
    for raw in body.split("\n"):
        s = raw.strip()
        m = re.match(r"^:?;\((.*?)\)$", s) or re.match(r"^::;\((.*?)\)$", s)
        if m:
            marker = "(" + m.group(1) + ")"
            if marker in EN_SCENES:
                scene = EN_SCENES[marker]
            continue
        m = re.match(r"^::'''([^']+?):'''\s*(.*)$", s)          # cinematic
        if not m:
            m = re.match(r"^:?\{\{A\|[^}]*\}\}\s*'''([^']+?):'''\s*(.*)$", s)
        if not m:
            m = re.match(r"^::\{\{A\|[^}]*\}\}\s*'''([^']+?):'''\s*(.*)$", s)
        if not m:
            continue
        who, text = m.group(1).strip(), plain(m.group(2))
        if who not in EN_SPEAKERS or not text:
            if who == "The Jester" and text.startswith('"') and text.endswith('"'):
                who, text = HIS_EN, unwrap_quote(text)          # notebook read aloud
            else:
                continue
        # Only the three places this canon is about: the PV, the seminar, and
        # the notebook. The rest of the quest is Pierro's present.
        if not (scene.startswith("Bygone Days") or "seminar under the tree" in scene
                or scene == "The Wise's notebook"):
            continue
        if who == "Traveler" and scene == "the seminar under the tree":
            scene = "the seminar under the tree — with the Traveler"
        out.append((scene, who, text))
    return out


def blocks(rows, header, colon):
    """Group consecutive rows of the same scene into [[scene] ...] blocks."""
    out, cur = [], None
    for scene, who, text in rows:
        if scene != cur:
            if cur is not None:
                out.append("")
            out.append(f"[{header}{scene}]")
            cur = scene
        out.append(f"{who}{colon}{text}")
    return out


def main():
    force = "--force" in sys.argv
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    dest = os.path.join(here, "app", "canon.txt")
    if os.path.exists(dest) and not force:
        print(f"{dest} exists — leaving it alone (use --force to rebuild)")
        return 1
    print("fetching zh …")
    zh = zh_lines(fetch(ZH_API, ZPAGE))
    print("fetching en …")
    en = en_lines(fetch(EN_API, EPAGE))

    body = ["# 贤者笔记 canon — 海洛塔帝的原话 / Hroptatyr's own lines.",
            "# Generated by scripts/fetch-canon.py from",
            "#   https://wiki.biligame.com/ys/被镌刻的时光",
            "#   https://genshin-impact.fandom.com/wiki/Etched_in_Time",
            "# Personal use only: the lines are HoYoverse's, not this project's.",
            "# Edit freely — 「原话」 lookup and the easter egg read this file.",
            ""]
    body += blocks(zh, "场景：", "：")
    body.append("")
    body += blocks(en, "Scene: ", ": ")
    body.append("")
    with open(dest, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(body))
    print(f"wrote {dest}: {len(zh)} zh rows, {len(en)} en rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
