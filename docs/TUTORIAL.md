# The Wise's Notes: the full tutorial

**English** · [简体中文](TUTORIAL.zh-CN.md) · [Back to the README](../README.md)

This guide takes you from a stock reMarkable Paper Pro to a notebook in which young Hroptatyr answers your handwriting. It covers installing, everyday use, choosing and configuring a model, memory, canon quotes, and what to do when something goes wrong.

- [1. What you need](#1-what-you-need)
- [2. Prepare the tablet](#2-prepare-the-tablet)
- [3. Prepare your computer](#3-prepare-your-computer)
- [4. Build and install](#4-build-and-install)
- [5. Give him a model](#5-give-him-a-model)
- [6. Choosing a model: recipes](#6-choosing-a-model-recipes)
- [7. Using the notebook](#7-using-the-notebook)
- [8. Memory, sittings and notes](#8-memory-sittings-and-notes)
- [9. Canon quotes (personal use only)](#9-canon-quotes-personal-use-only)
- [10. Fonts: changing his handwriting](#10-fonts-changing-his-handwriting)
- [11. Every setting](#11-every-setting)
- [12. Updating, backing up, uninstalling](#12-updating-backing-up-uninstalling)
- [13. Troubleshooting](#13-troubleshooting)
- [14. Privacy](#14-privacy)

---

## 1. What you need

| | |
|---|---|
| **Tablet** | A reMarkable Paper Pro with developer mode on, reachable over SSH. |
| **On the tablet** | [xovi](https://github.com/asivery/xovi) and [AppLoad](https://github.com/asivery/rm-appload), which put the notebook in the sidebar. |
| **Computer** | Linux, or Windows with [WSL](https://learn.microsoft.com/windows/wsl/install) (Ubuntu). macOS works too if you install the same tools. |
| **Tools** | [`rustup`](https://rustup.rs) with the `aarch64-unknown-linux-gnu` target, [`zig`](https://ziglang.org) (the simplest way is `pip install ziglang`), `python3`, `ssh`/`scp`, `curl`. |
| **A model** | Any **vision-capable** model: a cloud API key (OpenAI, Gemini, Claude, OpenRouter…) or a local server on your network (Ollama, LM Studio…). It reads your handwriting as an image, so text-only models will not do. |

> Developer mode and these tools change how your tablet runs. Read reMarkable's own notes on developer mode first, keep your SSH password somewhere safe, and learn the escape hatch in [section 13](#13-troubleshooting) before you start.

## 2. Prepare the tablet

1. **Developer mode.** Turn it on in the tablet's settings (reMarkable documents where it lives for your OS version). Enabling it resets the tablet, so sync your documents first.
2. **SSH.** With the USB cable connected, the tablet is at `10.11.99.1`. Its root password is shown in the settings, in the developer/about section. Check that it works:
   ```sh
   ssh root@10.11.99.1
   ```
3. **A short name.** The scripts reach the tablet as `rm`. Add this to `~/.ssh/config` on your computer (in WSL, that is WSL's home folder):
   ```
   Host rm
       HostName 10.11.99.1
       User root
   ```
   Over Wi-Fi, use the tablet's Wi-Fi address instead. For password-free logins, copy your key across with `ssh-copy-id rm`. To keep a different name, set `SAGE_HOST=yourname` when you run the scripts.
4. **xovi and AppLoad.** Install both by following their own instructions. When you are done, the AppLoad panel opens from the tablet's sidebar. That is where the notebook will appear.

## 3. Prepare your computer

In Linux or WSL:

```sh
# Rust and the tablet's target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-unknown-linux-gnu

# zig, used as the cross compiler and linker (no reMarkable SDK needed)
python3 -m pip install --user ziglang
# If pip refuses ("externally managed environment"), use uv or pipx instead and
# tell the build which Python has it, e.g. after `uv tool install ziglang`:
#   export SAGE_PYTHON=~/.local/share/uv/tools/ziglang/bin/python
# A zig binary on your PATH works too.

# the project
git clone <this repository> sage-notes
cd sage-notes
```

## 4. Build and install

The first build copies two libraries from your own tablet (they are not redistributed), runs the tests, and builds the binary:

```sh
./scripts/build.sh --pull-libs --test
```

Later builds only need `./scripts/build.sh`. The result is `dist/sage`.

Install it into AppLoad:

```sh
./scripts/deploy.sh
```

This copies the app to `/home/root/xovi/exthome/appload/sage-notes` (the tile with the Chinese guide) and adds a second tile, `sage-notes-en` (**The Wise's Notes**, with the English guide). Both tiles share one binary, one persona, one key and one memory. The first time, it also fetches the Chinese font with [`scripts/fetch-fonts.sh`](../scripts/fetch-fonts.sh) (about 25 MB).

On first install, `deploy.sh` creates `oracle.env` on the tablet. It never overwrites it again, so your key survives updates.

Open the AppLoad panel and refresh it. Both tiles should be there.

## 5. Give him a model

Without a model the notebook still opens, but the first page tells you it "lies dormant". Give it one in either of two ways.

**A. On the tablet, in AppLoad's settings for the app.** Pick a preset (OpenAI, Gemini, Claude, OpenRouter, Ollama), paste your key, and check the model name.

**B. By editing `oracle.env` over SSH:**

```sh
ssh rm
vi /home/root/xovi/exthome/appload/sage-notes/oracle.env
```

The minimum is a key:

```sh
SAGE_API_KEY=sk-...
```

That uses OpenAI with `gpt-4o-mini`. For anything else, set `SAGE_API_BASE` and `SAGE_MODEL` from one of the [recipes below](#6-choosing-a-model-recipes).

**Check it before you write a page.** This sends one test page ("Who are you?") and prints his streamed answer:

```sh
ssh rm SAGE_UI_LANG=en /home/root/xovi/exthome/appload/sage-notes/sage-cli.sh --oracle-test
```

`sage-cli.sh` runs the notebook's checks with the same settings the notebook uses (`oracle.env`, persona, fonts, canon), without touching the screen. Leave out `SAGE_UI_LANG=en` to send the Chinese test page instead. The test reads your memory but never writes to it.

You should see `first chunk +…ms`, then a few sentences in his voice, then `reply complete`. If not, the error line says why (see [section 13](#13-troubleshooting)).

## 6. Choosing a model: recipes

The notebook speaks the two wire formats that between them cover almost every provider:

| `SAGE_API` | Request | Speaks to |
|---|---|---|
| `openai` (default) | `POST {base}/chat/completions` | OpenAI, Gemini, OpenRouter, Groq, Mistral, xAI, Azure, Ollama, LM Studio, vLLM, llama.cpp |
| `anthropic` | `POST {base}/messages` | Claude |

When `SAGE_API` is unset it is guessed: a base on `anthropic.com` means `anthropic`, and anything else means `openai`.

What matters in a model, in order:
1. **It reads images**, including handwriting and Chinese characters.
2. **It is fast to the first word**, because the reply is inked as it streams.
3. **It follows instructions**: short replies, plus the transcription and note lines at the end.

Small, fast vision models are the sweet spot. Big thinking models are slower to start writing.

Model names change often. Treat the ones below as examples and check your provider's current list.

**OpenAI**
```sh
SAGE_API_KEY=sk-...
SAGE_MODEL=gpt-4o-mini
```

**Google Gemini** (through its [OpenAI-compatible endpoint](https://ai.google.dev/gemini-api/docs/openai))
```sh
SAGE_API_BASE=https://generativelanguage.googleapis.com/v1beta/openai
SAGE_API_KEY=AIza...
SAGE_MODEL=gemini-3.5-flash
SAGE_REASONING=low          # thinking models: start the ink sooner
```

**Anthropic Claude**
```sh
SAGE_API=anthropic
SAGE_API_KEY=sk-ant-...
SAGE_MODEL=claude-haiku-5-5
```

**[OpenRouter](https://openrouter.ai)** (one key for many providers)
```sh
SAGE_API_BASE=https://openrouter.ai/api/v1
SAGE_API_KEY=sk-or-...
SAGE_MODEL=openai/gpt-4o-mini
SAGE_API_HEADERS=X-Title: The Wise's Notes      # optional
```

**[Ollama](https://ollama.com) on your own computer** (no key, no cloud)
```sh
# on the computer:  ollama pull qwen2.5vl   and let it listen on the network:
#                   OLLAMA_HOST=0.0.0.0 ollama serve
SAGE_API_BASE=http://192.168.1.20:11434/v1     # your computer's address
SAGE_MODEL=qwen2.5vl
```

**[LM Studio](https://lmstudio.ai)** (no key): start its local server with a vision model loaded, and allow connections from the network.
```sh
SAGE_API_BASE=http://192.168.1.20:1234/v1
SAGE_MODEL=<the model id LM Studio shows>
```

**Any other OpenAI-compatible service** (Groq, Mistral, xAI, a company proxy, Azure's v1 endpoint…): set `SAGE_API_BASE` to everything before `/chat/completions`, then set `SAGE_API_KEY` and `SAGE_MODEL`. If the service wants its key in a different header (Azure's `api-key`, for example), put it in `SAGE_API_HEADERS`:
```sh
SAGE_API_HEADERS=api-key: your-azure-key
```

A key is required for any cloud address. It may be left out for addresses on your own network (`localhost`, `127.x`, `10.x`, `172.16–31.x`, `192.168.x`, `*.local`).

## 7. Using the notebook

**Opening.** Tap **The Wise's Notes** (English guide and messages) or the Chinese tile in AppLoad. The tablet's own screens step aside while the notebook is open.

**Writing.** Write anywhere on the page, in English or Chinese, then lift your pen. After a short pause your ink sinks into the paper, a few thinking marks appear, and his answer writes itself in ink. Then it fades and the page is yours again. He answers in the language you wrote in.

**What to write.** Anything: a question, a problem, a decision, a feeling, a line of code. He treats it as the topic of the seminar under the tree, finds its heart and answers, often with a question back. Some things that work well:

- Begin a page with `Topic:` for a longer, fuller answer.
- Write only "hello" or "I don't know what to ask", and he will propose a puzzle of his own.
- Tell him something from your world (phones, the internet, your job). He does not know the names, but he understands quickly.
- Challenge him ("I bet you can't solve this one") and he will wager you a tea.
- Mention things from after his time (the Fatui, the Sinners…) and see how he takes it.

**Bringing back a page.** Write "show me what I wrote about…". The old page rises again in your own handwriting, with its date and his old reply. Touch the page to return.

**Erasing.** Flip the pen and use its eraser end before you lift it. Erased ink is not sent.

**The guide.** Tap *guide* at the foot of the page, or draw one large **?**, for a summary of all this.

**Sleep.** Press the power button: a sleep card appears and the tablet sleeps. Press it again to wake. The notebook is right where you left it.

**Leaving.** Tap *close* at the foot of the page twice, or touch the screen with five fingers. The tablet's normal screens return.

**Refusals.** Ask for something harmful and the page "goes blank": the ink will not take, and he offers something else instead.

## 8. Memory, sittings and notes

**He remembers.** Every finished page is kept on the tablet (the newest 400): your strokes, a transcription, and his reply.

**Sittings.** Pages less than 45 minutes apart are one sitting (`SAGE_SITTING_GAP` changes this), and within a sitting he follows the conversation. After a longer break it is the same afternoon for him again, begun anew. He recognises your earlier pages "in your hand" and greets you as someone who has been here before, not as a stranger.

**Notes.** He keeps a few short notes about you: your name, how you like to be addressed, an open wager, a long-running project. He uses them the way you would glance at your own margin notes. To change them, just write it: "forget my name" removes a note, and "call me Sam" updates one. They are plain text in `/home/root/sage-data/memories/facts.tsv`, which you can read, edit or delete.

**Your time of day.** The tablet's clock keeps UTC. Set `SAGE_TZ_OFFSET` (for example `-5` for New York in winter, `1` for Berlin in winter, `8` for China) so dates read right and he knows when it is late for you. Without it he is never told the hour.

**Forgetting everything.** Delete `/home/root/sage-data/memories`. To keep nothing in the first place, set `SAGE_MEMORY=off`.

## 9. Canon quotes (personal use only)

Ask for his exact words ("what were your exact words?"), or write out a line from the quest, and the notebook quotes his lines from the quest *Etched in Time* word for word. It looks them up itself on the tablet, so the lines are never sent to the model and nothing is ever invented. When it does not have the line, he says the ink has faded.

> **Personal use only.** These lines are HoYoverse's copyrighted script text and are not part of this project. Fetch them only for your own notebook, and never share, commit, bundle or redistribute `canon.txt`.

```sh
./scripts/fetch-canon.py           # builds app/canon.txt from the two fan wikis
./scripts/deploy.sh --canon        # copies it to your tablet
```

The script fetches both the Chinese and the English dialogue, so he can quote in either language.

What works (tested against a full `canon.txt`):

| You write | You get |
|---|---|
| One of his lines, even with a slip, or with different wording | That line, plus the next one when it trails off |
| A line someone else said to him | What he said next |
| A place ("the professor's sickroom", "the gravestone in the rain") | His lines from that scene |
| Something that is not in the canon | An honest "I can't recall the exact words", never a made-up quote |

A quote is at most one page long. Very short fragments of one or two words are too short to count as a quotation, so ask with a phrase.

You can edit `canon.txt` freely. Each scene starts with a heading line, and each line of dialogue starts with the speaker's name. English and Chinese labels both work:

```
[Scene: the seminar under the tree]
Hroptatyr: (his exact line)
Eftirvit: (a line said to him)
```

To check what a phrase would match without calling a model, run this on the tablet:

```sh
ssh rm "/home/root/xovi/exthome/appload/sage-notes/sage-cli.sh --canon-test 'the gravestone in the rain'"
```

## 10. Fonts: changing his handwriting

His replies, the guide and the page frame are drawn from ordinary font files, traced into pen strokes. By default:

| Role | Setting | Default file | Used for |
|---|---|---|---|
| Latin hand | `SAGE_FONT` | `fonts/CrimsonPro-Italic.ttf` | English, numbers, Latin punctuation |
| Chinese hand | `SAGE_FONT_CJK` | `fonts/LXGWWenKai-Regular.ttf` (LXGW WenKai) | Chinese, full-width punctuation |
| More fallbacks | `SAGE_FONT_FALLBACK` | (none) | Anything the two above lack, e.g. Japanese kana or symbols |
| Last resort | (built in) | Dancing Script | Used when the Latin font cannot be loaded |

**How a character picks its font.** Each character is drawn with the first font that has it: `SAGE_FONT`, then `SAGE_FONT_CJK`, then each file in `SAGE_FONT_FALLBACK`, in order. So a Latin font without Chinese characters is fine, because Chinese falls through to the CJK font. A Chinese font that also has Latin letters only draws them if you put it in `SAGE_FONT`.

**Changing a font**

1. Pick a font file: TrueType `.ttf` or OpenType `.otf` (for a `.ttc` collection, only the first face is used). Check that its license allows your use. The bundled fonts are under the [SIL Open Font License](https://openfontlicense.org).
2. Copy it to the tablet:
   ```sh
   scp MyHand.ttf rm:/home/root/xovi/exthome/appload/sage-notes/fonts/
   ```
3. Point the setting at it in `oracle.env`. These lines override the launcher's defaults:
   ```sh
   SAGE_FONT=/home/root/xovi/exthome/appload/sage-notes/fonts/MyHand.ttf
   SAGE_FONT_CJK=/home/root/xovi/exthome/appload/sage-notes/fonts/MyKai.ttf
   SAGE_FONT_FALLBACK=/home/root/xovi/exthome/appload/sage-notes/fonts/A.otf:/home/root/xovi/exthome/appload/sage-notes/fonts/B.ttf
   ```
   `SAGE_FONT_FALLBACK` is a colon-separated list.
4. Close and reopen the notebook.

If a file cannot be read, the log says so (`journalctl -u sage-takeover`) and the notebook carries on with the remaining fonts, so a typo never leaves you without ink.

**What looks good.** Replies are traced into single pen strokes, so fonts with a clear, even stroke look most like handwriting: Kai styles for Chinese, and italic, script or humanist faces for Latin. Very thin hairline faces can break up, and very heavy or decorative faces lose their shapes. Large CJK fonts (tens of MB) work but take a moment longer to load when the notebook opens.

**Preview on your computer first.** Without touching the tablet, render the empty page, the guide and the thinking marks with your fonts:

```sh
SAGE_FONT=fonts/MyHand.ttf SAGE_FONT_CJK=fonts/LXGWWenKai-Regular.ttf SAGE_UI_LANG=en \
  cargo test preview_ui -- --ignored
```

This writes `sage-preview-blank.png`, `sage-preview-guide.png` and `sage-preview-thinking.png` to your temp folder (`/tmp` on Linux and WSL).

**Making it stick across updates.** `deploy.sh` refreshes the bundled fonts but never touches your own files or `oracle.env`, so a custom font survives updates. To ship a different default to every install instead, replace the file in `fonts/` and adjust `SAGE_FONT`/`SAGE_FONT_CJK` in [`app/sage-takeover.sh`](../app/sage-takeover.sh) and [`app/sage-cli.sh`](../app/sage-cli.sh).

**Other ways to change the look**

- **The title** in the guide: `SAGE_TITLE=…`
- **The language** of the guide, sleep card and messages: `SAGE_UI_LANG=en` or `zh`. Each tile sets its own, and he always answers in the language you write in.
- **The persona** (his voice, the length of his replies, what he knows): edit `persona.txt` on the tablet. It takes effect on the next page, with no rebuild.

## 11. Every setting

All settings are `SAGE_*` lines in `oracle.env`, next to the binary on the tablet. Edit them there or in AppLoad's settings. Changes take effect the next time you open the notebook. A commented example is in [app/oracle.env.example](../app/oracle.env.example).

**The model**

| Setting | Default | |
|---|---|---|
| `SAGE_API_KEY` | (none) | Your key. Not needed for a local server. |
| `SAGE_API` | guessed | `openai` or `anthropic`. |
| `SAGE_API_BASE` | `https://api.openai.com/v1` (or `https://api.anthropic.com/v1`) | Everything before `/chat/completions` or `/messages`. |
| `SAGE_MODEL` | `gpt-4o-mini` (or `claude-haiku-5-5`) | Must read images. |
| `SAGE_MAX_TOKENS` | `2000` | Runaway guard. Keep it roomy for thinking models. |
| `SAGE_REASONING` | (unset) | `low`/`medium`/`high`, sent as `reasoning_effort` (`openai` only). |
| `SAGE_API_HEADERS` | (none) | Extra headers, `Name: value; Other: value`. |
| `SAGE_API_TIMEOUT` | `90` | Seconds of silence allowed while the reply streams. |

The names from earlier versions (`SAGE_OPENAI_KEY`, `SAGE_OPENAI_BASE`, `SAGE_OPENAI_MODEL`, `SAGE_OPENAI_MAX_TOKENS`, `SAGE_OPENAI_REASONING`) still work. When both are set, the new name wins.

**The notebook**

| Setting | Default | |
|---|---|---|
| `SAGE_PERSONA_FILE` | `persona.txt` in the app folder | Who he is. Plain text you can edit on the tablet; no rebuild needed. |
| `SAGE_CANON_FILE` | `canon.txt` in the app folder | Your exact lines. |
| `SAGE_CANON_MODE` | `lookup` | `inline` sends the whole canon with every page instead (slower and dearer). |
| `SAGE_UI_LANG` | `zh` for the Chinese tile, `en` for The Wise's Notes | Language of the guide, sleep card and messages. |
| `SAGE_TITLE` | tile name | Title in the guide. |
| `SAGE_FONT`, `SAGE_FONT_CJK`, `SAGE_FONT_FALLBACK` | bundled fonts | His Latin and Chinese hands, and extra fallbacks. |

**Memory**

| Setting | Default | |
|---|---|---|
| `SAGE_MEMORY` | on | `off` keeps nothing and sends no history. |
| `SAGE_MEMORY_DIR` | `/home/root/sage-data/memories` | Pages and notes. |
| `SAGE_MEMORY_TURNS` | `6` | Most pages of the current sitting sent along. |
| `SAGE_SITTING_GAP` | `45` | Minutes of silence that end a sitting. |
| `SAGE_TZ_OFFSET` | (unset) | Hours from UTC. |

**Debugging**

| Setting | |
|---|---|
| `SAGE_KEEP_PAGE=1` | Keep `/tmp/sage-page.png`, the image of your last page as sent to the model. |

## 12. Updating, backing up, uninstalling

- **Update:** `git pull`, then `./scripts/build.sh` and `./scripts/deploy.sh`. Your `oracle.env`, `canon.txt` and memories are kept.
- **Back up your memories:**
  ```sh
  scp -r rm:/home/root/sage-data/memories ./memories-backup
  ```
- **Uninstall:**
  ```sh
  ssh rm 'rm -rf /home/root/xovi/exthome/appload/sage-notes /home/root/xovi/exthome/appload/sage-notes-en'
  ```
  Also delete `/home/root/sage-data` if you want the memories gone.

## 13. Troubleshooting

**The screen seems frozen.** First touch the screen with five fingers: that always closes the notebook. If that does not work, connect the cable and run:
```sh
ssh rm 'systemctl reset-failed xochitl; systemctl start xochitl'
```

**Reading the log.** The notebook runs as a systemd unit, and its log lines start with `sage:`:
```sh
ssh rm 'journalctl -u sage-takeover -n 80 --no-pager'
```
The first lines show which API, URL and model it chose (`sage: oracle api=… url=… model=…`).

| What the page says | What it means | What to do |
|---|---|---|
| "The notebook lies dormant…" | No key for a cloud address. | Set `SAGE_API_KEY`, or point `SAGE_API_BASE` at a local server. |
| "The oracle refused the notebook's key…" | HTTP 401/403. | The key is wrong, expired, or for another provider. |
| "The oracle rejected the notebook's plea (http 400/404…)…" | The model name or base URL is wrong, or the model cannot read images. | Check `SAGE_MODEL` and `SAGE_API_BASE` against your provider. Run `--oracle-test` to see the server's message. |
| "The notebook cannot reach its oracle. Is the tablet connected to Wi-Fi?" | No network for 40 seconds. | Check Wi-Fi. For a local server, check its address and that it listens on the network, not just on localhost. |
| "He read your words but wrote nothing…" | The model sent an empty reply. | Usually `SAGE_MAX_TOKENS` is too low for a thinking model. Raise it, or set `SAGE_REASONING=low`. |
| "The ink smudged before he could answer…" | Anything else. | Read the log. |

**After waking from sleep.** Wi-Fi takes a few seconds to reconnect. The notebook waits and retries for up to 40 seconds before giving up, so a page written right after waking may take a moment longer. The press that wakes the tablet never sends it back to sleep. If sleep itself fails (the screen's power supply can refuse for up to 30 seconds after a refresh), it retries a few times and then shows the page again.

**Slow first words.** Pick a smaller or faster model, set `SAGE_REASONING=low` for a thinking model, or use a server closer to you.

**He doesn't call me by name.** Tell him your name on a page, then check `facts.tsv` to see what he noted.

## 14. Privacy

- **What leaves the tablet**, and only to the model address you configured: an image of the ink you just wrote, the persona, the current sitting's earlier pages (as text), a short list of your older pages' first words with their dates, and his notes about you.
- **What stays on the tablet:** everything else, including your strokes, transcripts, replies, notes and `canon.txt`. Nothing is uploaded anywhere else.
- He is told never to note health, money, home addresses, passwords or similar details unless you ask him to remember them.
- Your key is stored only in `oracle.env` on the tablet (readable by root only).
- With a local server (Ollama, LM Studio), nothing leaves your own network at all.
