# Acknowledgements

## riddle, by Maxime Rivest
贤者笔记 began as a fork of **[riddle](https://github.com/MaximeRivest/riddle)** (MIT), the "Tom Riddle's diary" for the reMarkable Paper Pro. Starting point: commit `0d1a9fe`, July 2026. riddle came up with the core idea and engine: handwriting read by a vision model, replies inked stroke by stroke, and a memory of pages that can be conjured back. Most of `src/` still descends from it, and the original copyright notice is kept in [LICENSE](LICENSE). We plan to rework much of it; thank you, Maxime.

Changed or added so far:
- A CJK font-fallback hand, CJK line breaking with kinsoku, and sentence streaming at full-width stops.
- The system prompt is loaded from a file, the built-in persona is Hroptatyr, and an optional canon file holds exact quotes, looked up on the device rather than sent to the model.
- One streaming client for both the OpenAI and the Anthropic APIs, replacing riddle's two backends (the pi process is gone).
- Memory in sittings, with a one-line recap of the last sitting and a small set of notes about the writer.
- Chinese UI strings and Chinese dates on conjured pages.
- `SAGE_*` configuration, its own AppLoad bundle, icon and build scripts.
- A self-contained release zip (`scripts/make-bundle.sh`) that needs nothing else on the tablet.

## quill
On-device takeover rendering goes through **[quill](https://github.com/MaximeRivest/quill)** (MIT), the clean-room C ABI to the reMarkable e-ink engine, through `libquill.so`. The release zip ships the prebuilt `libquill.so` from riddle's v0.3.0 release, unmodified, with quill's license in [licenses/quill-LICENSE.txt](licenses/quill-LICENSE.txt). On OS 3.29 it runs with `libquillcompat.so`, a swapBuffers shim written for this project ([compat/quillcompat.c](compat/quillcompat.c), MIT). reMarkable's own `libqsgepaper.so` is never redistributed; builds link against the copy on your own tablet.

## Fonts (SIL Open Font License 1.1)
- **LXGW WenKai 霞鹜文楷** by LXGW: the Chinese hand.
- **Crimson Pro** by Jacques Le Bailly: the Latin hand.
- **Dancing Script** by Impallari Type: the fallback hand, inherited from riddle.

## Genshin Impact
Hroptatyr (海洛塔帝), Khaenri'ah and every related name are © HoYoverse. This is an unofficial, non-commercial fan project with no affiliation to HoYoverse. It ships no game art, audio or script text. The persona is written in original words. Exact game lines appear only in the user's own `canon.txt`, which `scripts/fetch-canon.py` builds on the user's machine from public fan wikis for personal use only; it is git-ignored and is never committed, bundled or redistributed. The icon is an original drawing, made with `scripts/make-icon.py`, inspired by the notebook he carries in the game.
