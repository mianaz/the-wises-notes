# 贤者笔记 完整教程

[English](TUTORIAL.md) · **简体中文** · [返回说明](../README.zh-CN.md)

本教程带你从一台出厂状态的 reMarkable Paper Pro 开始，一步步得到一本由年轻的海洛塔帝回应你笔迹的笔记。内容包括安装、日常使用、选择与配置模型、记忆、原话引用，以及出问题时该怎么办。

- [1. 准备工作](#1-准备工作)
- [2. 准备平板](#2-准备平板)
- [3. 准备电脑](#3-准备电脑)
- [4. 编译与安装](#4-编译与安装)
- [5. 给他配置模型](#5-给他配置模型)
- [6. 选择模型与配置示例](#6-选择模型与配置示例)
- [7. 使用笔记](#7-使用笔记)
- [8. 记忆、落座与笔记](#8-记忆落座与笔记)
- [9. 原话引用（仅限个人使用）](#9-原话引用仅限个人使用)
- [10. 字体与他的笔迹](#10-字体与他的笔迹)
- [11. 全部设置](#11-全部设置)
- [12. 更新、备份与卸载](#12-更新备份与卸载)
- [13. 故障排查](#13-故障排查)
- [14. 隐私](#14-隐私)

---

## 1. 准备工作

| | |
|---|---|
| **平板** | 一台开启了开发者模式、能通过 SSH 连接的 reMarkable Paper Pro。 |
| **平板上的软件** | [xovi](https://github.com/asivery/xovi) 和 [AppLoad](https://github.com/asivery/rm-appload)，它们让贤者笔记出现在侧边栏里。 |
| **电脑** | Linux，或装有 [WSL](https://learn.microsoft.com/zh-cn/windows/wsl/install)（Ubuntu）的 Windows。macOS 装好同样的工具也可以。 |
| **工具** | 带 `aarch64-unknown-linux-gnu` 目标的 [`rustup`](https://rustup.rs)、[`zig`](https://ziglang.org)（最简单的方式是 `pip install ziglang`）、`python3`、`ssh`/`scp`、`curl`。 |
| **一个模型** | 任何**支持图像输入**的模型：云端 API 密钥（OpenAI、Gemini、Claude、OpenRouter……），或你局域网内的本地服务器（Ollama、LM Studio……）。它需要把你的手写当作图片来读，所以纯文本模型不行。 |

> 开发者模式和这些工具会改变平板的运行方式。开始之前，请先阅读 reMarkable 官方关于开发者模式的说明，妥善保存 SSH 密码，并了解[第 13 节](#13-故障排查)中的应急办法。

## 2. 准备平板

1. **开发者模式。** 在平板的设置中开启（reMarkable 官方文档说明了它在你的系统版本中的位置）。开启会重置平板，请先同步好你的文档。
2. **SSH。** 用 USB 线连接后，平板的地址是 `10.11.99.1`。root 密码显示在设置中的开发者／关于页面。先确认能连上：
   ```sh
   ssh root@10.11.99.1
   ```
3. **起一个短名字。** 脚本通过 `rm` 这个名字连接平板。在电脑的 `~/.ssh/config` 中加入以下内容（如果用 WSL，就是 WSL 里的家目录）：
   ```
   Host rm
       HostName 10.11.99.1
       User root
   ```
   通过 Wi-Fi 连接时，换成平板的 Wi-Fi 地址。想免密码登录，可以用 `ssh-copy-id rm` 把公钥复制过去。如果想用别的名字，运行脚本时设置 `SAGE_HOST=你的名字` 即可。
4. **xovi 与 AppLoad。** 按照它们各自的说明安装。装好后，从平板侧边栏可以打开 AppLoad 面板，贤者笔记就会出现在那里。

## 3. 准备电脑

在 Linux 或 WSL 中：

```sh
# Rust 及平板的编译目标
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-unknown-linux-gnu

# zig，用作交叉编译器和链接器（不需要 reMarkable SDK）
python3 -m pip install --user ziglang
# 如果 pip 拒绝安装（提示 "externally managed environment"），改用 uv 或 pipx，
# 并告诉构建脚本哪个 Python 里装了它，例如执行 `uv tool install ziglang` 之后：
#   export SAGE_PYTHON=~/.local/share/uv/tools/ziglang/bin/python
# PATH 中已有 zig 程序也可以。

# 获取项目
git clone <本仓库地址> sage-notes
cd sage-notes
```

## 4. 编译与安装

第一次编译会从你自己的平板复制两个库（它们不会被再分发），运行测试，然后编译程序：

```sh
./scripts/build.sh --pull-libs --test
```

之后再编译只需要 `./scripts/build.sh`。生成的程序是 `dist/sage`。

安装到 AppLoad：

```sh
./scripts/deploy.sh
```

这会把应用复制到 `/home/root/xovi/exthome/appload/sage-notes`（**贤者笔记**，中文指引），并添加第二块磁贴 `sage-notes-en`（`The Wise's Notes`，英文指引）。两块磁贴共用同一个程序、同一份人设、同一个密钥和同一份记忆。第一次安装时，它还会通过 [`scripts/fetch-fonts.sh`](../scripts/fetch-fonts.sh) 下载中文字体（约 25 MB）。

首次安装时，`deploy.sh` 会在平板上创建 `oracle.env`。之后它永远不会覆盖这个文件，所以更新时你的密钥会保留下来。

打开 AppLoad 面板并刷新，两块磁贴就会出现。

## 5. 给他配置模型

没有模型时笔记也能打开，但第一页会告诉你「笔记沉睡着」。有两种方式给它配置模型。

**方式 A：在平板上，进入 AppLoad 中贤者笔记的设置。** 选择一个预设（OpenAI、Gemini、Claude、OpenRouter、Ollama），粘贴密钥，再确认模型名称。

**方式 B：通过 SSH 编辑 `oracle.env`：**

```sh
ssh rm
vi /home/root/xovi/exthome/appload/sage-notes/oracle.env
```

最少只需要一个密钥：

```sh
SAGE_API_KEY=sk-...
```

这样会使用 OpenAI 的 `gpt-4o-mini`。想用别的模型，请参照[下面的配置示例](#6-选择模型与配置示例)设置 `SAGE_API_BASE` 和 `SAGE_MODEL`。

**写第一页之前，先测试一下。** 下面的命令会发送一张测试页（「你是谁？」），并打印出他流式返回的回答：

```sh
ssh rm /home/root/xovi/exthome/appload/sage-notes/sage-cli.sh --oracle-test
```

`sage-cli.sh` 使用和笔记完全相同的设置（`oracle.env`、人设、字体、原话文件）运行检查，但不会动到屏幕。测试会读取你的记忆，但从不写入。

正常情况下你会看到 `first chunk +…ms`，然后是几句他口吻的话，最后是 `reply complete`。如果没有，报错的那一行会说明原因（见[第 13 节](#13-故障排查)）。

## 6. 选择模型与配置示例

笔记支持两种接口格式，几乎覆盖了所有服务商：

| `SAGE_API` | 请求 | 适用于 |
|---|---|---|
| `openai`（默认） | `POST {base}/chat/completions` | OpenAI、Gemini、OpenRouter、Groq、Mistral、xAI、Azure、Ollama、LM Studio、vLLM、llama.cpp |
| `anthropic` | `POST {base}/messages` | Claude |

不设置 `SAGE_API` 时会自动判断：地址在 `anthropic.com` 上就用 `anthropic`，其他一律用 `openai`。

挑选模型时，按重要程度依次看：
1. **能读图片**，包括手写字和汉字。
2. **出第一个字要快**，因为回答是边生成边书写的。
3. **听从指令**：回答要简短，并在末尾附上转写和笔记行。

小而快的视觉模型最合适。大型思考模型开始书写会比较慢。

模型名称经常变化。下面的名称仅作示例，请以服务商当前的列表为准。

**OpenAI**
```sh
SAGE_API_KEY=sk-...
SAGE_MODEL=gpt-4o-mini
```

**Google Gemini**（通过它的 [OpenAI 兼容接口](https://ai.google.dev/gemini-api/docs/openai)）
```sh
SAGE_API_BASE=https://generativelanguage.googleapis.com/v1beta/openai
SAGE_API_KEY=AIza...
SAGE_MODEL=gemini-3.5-flash
SAGE_REASONING=low          # 思考模型：让墨迹更早出现
```

**Anthropic Claude**
```sh
SAGE_API=anthropic
SAGE_API_KEY=sk-ant-...
SAGE_MODEL=claude-haiku-5-5
```

**[OpenRouter](https://openrouter.ai)**（一个密钥用多家服务商）
```sh
SAGE_API_BASE=https://openrouter.ai/api/v1
SAGE_API_KEY=sk-or-...
SAGE_MODEL=openai/gpt-4o-mini
SAGE_API_HEADERS=X-Title: The Wise's Notes      # 可选
```

**在你自己电脑上运行的 [Ollama](https://ollama.com)**（无需密钥，不经过云端）
```sh
# 在电脑上：ollama pull qwen2.5vl  并让它在局域网上监听：
#           OLLAMA_HOST=0.0.0.0 ollama serve
SAGE_API_BASE=http://192.168.1.20:11434/v1     # 换成你电脑的地址
SAGE_MODEL=qwen2.5vl
```

**[LM Studio](https://lmstudio.ai)**（无需密钥）：加载一个视觉模型后启动本地服务器，并允许来自局域网的连接。
```sh
SAGE_API_BASE=http://192.168.1.20:1234/v1
SAGE_MODEL=<LM Studio 中显示的模型 id>
```

**其他兼容 OpenAI 的服务**（Groq、Mistral、xAI、公司内部代理、Azure 的 v1 接口……）：把 `SAGE_API_BASE` 设为 `/chat/completions` 之前的全部地址，再设置 `SAGE_API_KEY` 和 `SAGE_MODEL`。如果服务要求把密钥放在别的请求头里（比如 Azure 的 `api-key`），写进 `SAGE_API_HEADERS`：
```sh
SAGE_API_HEADERS=api-key: 你的-azure-密钥
```

云端地址都需要密钥。你局域网内的地址（`localhost`、`127.x`、`10.x`、`172.16–31.x`、`192.168.x`、`*.local`）可以不填。

## 7. 使用笔记

**打开。** 在 AppLoad 中轻点**贤者笔记**（中文指引和提示）或 `The Wise's Notes`（英文）。笔记打开期间，平板原本的界面会暂时让开。

**书写。** 在页面任意位置用中文或英文书写，然后抬笔。稍等片刻，你的墨迹会沉入纸中，出现几笔思考的痕迹，接着他的回答会以墨迹自行写出。随后字迹淡去，页面又归你了。他用你书写的语言回答。

**写什么。** 什么都可以：一个问题、一道难题、一个抉择、一种心情、一行代码。他会把它当作「树下研讨会」的论题，找到核心并作答，常常还会反问你。以下写法效果很好：

- 在页首写「论题：」，可以得到更长、更完整的回答。
- 只写「你好」或「我不知道该问什么」，他会提出一道自己的谜题。
- 告诉他你世界里的东西（手机、互联网、你的工作）。他不认识这些名字，但很快就能明白。
- 向他挑战（「这道题你肯定解不出来」），他会跟你赌一杯茶。
- 提起他那个时代之后的事（愚人众、罪人……），看看他作何反应。

**找回旧页面。** 写「给我看看我写过的……」。旧页面会以你自己的笔迹重新浮现，带着当时的日期和他当时的回答。轻触页面即可返回。

**擦除。** 抬笔之前，把笔翻过来用橡皮端擦掉。擦掉的墨迹不会被发送。

**指引。** 轻点页脚的「指引」，或画一个大大的 **?**，就能看到以上内容的摘要。

**休眠。** 按电源键，会出现一张休眠卡片，平板进入休眠。再按一次即可唤醒，笔记还停在你离开的地方。

**离开。** 连点两下页脚的「合上」，或用五根手指同时触碰屏幕。平板原本的界面就会回来。

**拒绝。** 如果请求有害的内容，页面会「变成一片空白」：墨迹写不上去，他会提议换个话题。

## 8. 记忆、落座与笔记

**他会记得。** 每一页写完的内容都保存在平板上（保留最近的 400 页）：你的笔迹、一份转写和他的回答。

**落座。** 间隔不到 45 分钟的几页算作一次落座（可以用 `SAGE_SITTING_GAP` 修改），同一次落座里他会接着对话往下聊。隔得久了，对他来说又是同一个午后重新开始。他会认出你以前留下的笔迹，把你当作来过这里的人，而不是陌生人。

**笔记。** 他会记几条关于你的短笔记：你的名字、你喜欢的称呼、一个还没了结的赌约、一个长期进行的项目。他看这些笔记的方式，就像你瞥一眼自己页边的批注。想修改，直接写出来就行：「别再记我的名字了」会删掉一条，「叫我小明」会更新一条。它们以纯文本保存在 `/home/root/sage-data/memories/facts.tsv`，你可以随时查看、编辑或删除。

**你的时间。** 平板的时钟使用 UTC。请设置 `SAGE_TZ_OFFSET`（中国填 `8`），这样日期才正确，他也知道你那边是不是已经很晚了。不设置的话，他永远不会被告知时间。

**彻底遗忘。** 删除 `/home/root/sage-data/memories` 即可。如果一开始就什么都不想保留，设置 `SAGE_MEMORY=off`。

## 9. 原话引用（仅限个人使用）

问他的原话（例如「你当时的原话是什么？」），或者把任务里的一句台词写出来，笔记就会一字不差地引用他在「被镌刻的时光」中说过的话。台词由笔记自己在平板上查找，从不发给模型，也绝不编造。找不到时，他会说墨迹已经淡了。

> **仅限个人使用。** 这些台词是 HoYoverse 享有版权的剧本文字，不属于本项目。请只为你自己的笔记获取它们，永远不要分享、提交、打包或再分发 `canon.txt`。

```sh
./scripts/fetch-canon.py           # 从两个玩家 Wiki 生成 app/canon.txt
./scripts/deploy.sh --canon        # 复制到你的平板
```

脚本会同时抓取中文和英文对白，所以他两种语言都能引用。

效果如下（已用完整的 `canon.txt` 测试）：

| 你写 | 你得到 |
|---|---|
| 他说过的一句话，哪怕错了一个字，或用英文换了种说法 | 那句原话；如果话没说完，再接上下一句（「我要你…」→「…把下个月的研究经费…」） |
| 别人对他说的一句话（「艾弗提尔维特：你说你找到办法了？」） | 他接下来说的话 |
| 一个场景（「老师的病房」「雨中的墓碑」） | 他在那个场景里说的话 |
| 原话里没有的内容 | 一句坦白的「原话我记不清了」，绝不编造 |

每次引用最多一页。两个字的片段（比如「不死」）太短，不算引用，请用一个短语来问。

你可以随意编辑 `canon.txt`：`[场景：…]` 开始一个场景，`海洛塔帝：…` 是他的一句话，其他人的名字开头则是那个人的台词。想在不调用模型的情况下检查某个短语会匹配到什么，在平板上运行：

```sh
ssh rm '/home/root/xovi/exthome/appload/sage-notes/sage-cli.sh --canon-test 当心，要下雨了'
```

## 10. 字体与他的笔迹

他的回答、指引和页面边框，都是用普通字体文件描成笔画画出来的。默认设置如下：

| 用途 | 设置项 | 默认文件 | 用于 |
|---|---|---|---|
| 西文笔迹 | `SAGE_FONT` | `fonts/CrimsonPro-Italic.ttf` | 英文、数字、西文标点 |
| 中文笔迹 | `SAGE_FONT_CJK` | `fonts/LXGWWenKai-Regular.ttf`（霞鹜文楷） | 中文、全角标点 |
| 更多后备 | `SAGE_FONT_FALLBACK` | （无） | 上面两种字体都没有的字符，比如日文假名或符号 |
| 最后的后备 | （内置） | Dancing Script | 西文字体无法加载时使用 |

**每个字符如何选字体。** 每个字符都用第一个包含它的字体来写，顺序是 `SAGE_FONT`、`SAGE_FONT_CJK`，然后依次是 `SAGE_FONT_FALLBACK` 中的每个文件。所以西文字体里没有汉字也没关系，汉字会落到中文字体上。反过来，一款同时带有西文字母的中文字体，只有放在 `SAGE_FONT` 里时才会用它来写西文。

**更换字体**

1. 选一个字体文件：TrueType `.ttf` 或 OpenType `.otf`（`.ttc` 字体集只会用第一个字形）。确认它的许可允许你这样使用。附带的字体均采用 [SIL 开放字体许可证](https://openfontlicense.org)。
2. 复制到平板：
   ```sh
   scp MyHand.ttf rm:/home/root/xovi/exthome/appload/sage-notes/fonts/
   ```
3. 在 `oracle.env` 中指向它。这些设置会覆盖启动脚本的默认值：
   ```sh
   SAGE_FONT=/home/root/xovi/exthome/appload/sage-notes/fonts/MyHand.ttf
   SAGE_FONT_CJK=/home/root/xovi/exthome/appload/sage-notes/fonts/MyKai.ttf
   SAGE_FONT_FALLBACK=/home/root/xovi/exthome/appload/sage-notes/fonts/A.otf:/home/root/xovi/exthome/appload/sage-notes/fonts/B.ttf
   ```
   `SAGE_FONT_FALLBACK` 是用冒号分隔的列表。
4. 关闭笔记再重新打开。

如果某个文件读不了，日志里会写明（`journalctl -u sage-takeover`），笔记会用其余字体继续工作，所以写错路径也不会让你没有墨迹可用。

**什么样的字体好看。** 回答会被描成单一的笔画，所以笔画清晰、粗细均匀的字体最像手写：中文选楷体，西文选斜体、手写体或人文主义风格的字体。极细的发丝体可能断笔，过粗或装饰性太强的字体会失去形状。几十 MB 的大型中文字体也能用，只是打开笔记时加载会稍慢一些。

**先在电脑上预览。** 不用动平板，就能用你的字体渲染出空白页、指引页和思考痕迹：

```sh
SAGE_FONT=fonts/MyHand.ttf SAGE_FONT_CJK=fonts/LXGWWenKai-Regular.ttf SAGE_UI_LANG=zh \
  cargo test preview_ui -- --ignored
```

这会把 `sage-preview-blank.png`、`sage-preview-guide.png` 和 `sage-preview-thinking.png` 写到你的临时目录（Linux 和 WSL 上是 `/tmp`）。

**让更改在更新后保留。** `deploy.sh` 会刷新附带的字体，但从不碰你自己的文件和 `oracle.env`，所以自定义字体在更新后依然有效。如果想让每次安装都默认使用另一款字体，替换 `fonts/` 中的文件，并修改 [`app/sage-takeover.sh`](../app/sage-takeover.sh) 和 [`app/sage-cli.sh`](../app/sage-cli.sh) 中的 `SAGE_FONT`/`SAGE_FONT_CJK`。

**其他改变外观的方式**

- **指引中的标题：** `SAGE_TITLE=…`
- **指引、休眠卡片和提示的语言：** `SAGE_UI_LANG=zh` 或 `en`。每块磁贴各自设置，而他总是用你书写的语言回答。
- **人设**（他的语气、回答的长度、他知道什么）：编辑平板上的 `persona.txt`。下一页就会生效，无需重新编译。

## 11. 全部设置

所有设置都是 `oracle.env` 中的 `SAGE_*` 行，这个文件在平板上，与程序放在同一目录。可以直接编辑，也可以在 AppLoad 的设置中修改。更改在下次打开笔记时生效。带注释的示例见 [app/oracle.env.example](../app/oracle.env.example)。

**模型**

| 设置项 | 默认值 | 说明 |
|---|---|---|
| `SAGE_API_KEY` | （无） | 你的密钥。本地服务器不需要。 |
| `SAGE_API` | 自动判断 | `openai` 或 `anthropic`。 |
| `SAGE_API_BASE` | `https://api.openai.com/v1`（或 `https://api.anthropic.com/v1`） | `/chat/completions` 或 `/messages` 之前的全部地址。 |
| `SAGE_MODEL` | `gpt-4o-mini`（或 `claude-haiku-5-5`） | 必须能读图片。 |
| `SAGE_MAX_TOKENS` | `2000` | 防止回答失控的上限。用思考模型时请留足余量。 |
| `SAGE_REASONING` | （不设置） | `low`/`medium`/`high`，作为 `reasoning_effort` 发送（仅限 `openai`）。 |
| `SAGE_API_HEADERS` | （无） | 额外的请求头，格式为 `Name: value; Other: value`。 |
| `SAGE_API_TIMEOUT` | `90` | 流式回答时允许的最长静默秒数。 |

旧版本的设置名（`SAGE_OPENAI_KEY`、`SAGE_OPENAI_BASE`、`SAGE_OPENAI_MODEL`、`SAGE_OPENAI_MAX_TOKENS`、`SAGE_OPENAI_REASONING`）仍然有效。新旧名同时设置时，以新名为准。

**笔记**

| 设置项 | 默认值 | 说明 |
|---|---|---|
| `SAGE_PERSONA_FILE` | 应用目录中的 `persona.txt` | 他是谁。可在平板上直接编辑的纯文本，无需重新编译。 |
| `SAGE_CANON_FILE` | 应用目录中的 `canon.txt` | 你的原话文件。 |
| `SAGE_CANON_MODE` | `lookup` | 设为 `inline` 时，每一页都会附上整份原话（更慢，也更贵）。 |
| `SAGE_UI_LANG` | 贤者笔记为 `zh`，The Wise's Notes 为 `en` | 指引、休眠卡片和提示的语言。 |
| `SAGE_TITLE` | 磁贴名称 | 指引中的标题。 |
| `SAGE_FONT`、`SAGE_FONT_CJK`、`SAGE_FONT_FALLBACK` | 附带的字体 | 他的西文与中文笔迹，以及额外的后备字体。 |

**记忆**

| 设置项 | 默认值 | 说明 |
|---|---|---|
| `SAGE_MEMORY` | 开启 | 设为 `off` 时不保存任何内容，也不发送历史。 |
| `SAGE_MEMORY_DIR` | `/home/root/sage-data/memories` | 页面与笔记的存放位置。 |
| `SAGE_MEMORY_TURNS` | `6` | 当前落座中最多随请求发送的页数。 |
| `SAGE_SITTING_GAP` | `45` | 静默多少分钟后结束一次落座。 |
| `SAGE_TZ_OFFSET` | （不设置） | 与 UTC 相差的小时数。 |

**调试**

| 设置项 | 说明 |
|---|---|
| `SAGE_KEEP_PAGE=1` | 保留 `/tmp/sage-page.png`，即你上一页发送给模型时的图片。 |

## 12. 更新、备份与卸载

- **更新：** 先 `git pull`，再运行 `./scripts/build.sh` 和 `./scripts/deploy.sh`。你的 `oracle.env`、`canon.txt` 和记忆都会保留。
- **备份记忆：**
  ```sh
  scp -r rm:/home/root/sage-data/memories ./memories-backup
  ```
- **卸载：**
  ```sh
  ssh rm 'rm -rf /home/root/xovi/exthome/appload/sage-notes /home/root/xovi/exthome/appload/sage-notes-en'
  ```
  如果连记忆也不想保留，再删除 `/home/root/sage-data`。

## 13. 故障排查

**屏幕好像卡住了。** 先用五根手指同时触碰屏幕，这总能关闭笔记。如果不管用，接上 USB 线并运行：
```sh
ssh rm 'systemctl reset-failed xochitl; systemctl start xochitl'
```

**查看日志。** 笔记以 systemd 服务的形式运行，日志行以 `sage:` 开头：
```sh
ssh rm 'journalctl -u sage-takeover -n 80 --no-pager'
```
最前面几行会显示它选用的 API、地址和模型（`sage: oracle api=… url=… model=…`）。

| 页面上写着 | 含义 | 怎么办 |
|---|---|---|
| 「笔记沉睡着，找不到回应的人……」 | 云端地址没有配置密钥。 | 设置 `SAGE_API_KEY`，或把 `SAGE_API_BASE` 指向本地服务器。 |
| 「回应者拒绝了笔记的钥匙……」 | HTTP 401/403。 | 密钥错误、已过期，或属于另一家服务商。 |
| 「回应者拒绝了这次请求（http 400/404……）……」 | 模型名或地址不对，或者该模型不能读图片。 | 对照服务商文档检查 `SAGE_MODEL` 和 `SAGE_API_BASE`。运行 `--oracle-test` 查看服务器返回的信息。 |
| 「笔记联系不到回应者。平板连上 Wi-Fi 了吗？」 | 40 秒内没有网络。 | 检查 Wi-Fi。如果是本地服务器，检查地址是否正确，以及它是否在局域网上监听，而不只是监听 localhost。 |
| 「他读到了你的字，却什么也没写……」 | 模型返回了空回答。 | 通常是 `SAGE_MAX_TOKENS` 对思考模型来说太小。调大它，或设置 `SAGE_REASONING=low`。 |
| 「墨迹在回应之前晕开了……」 | 其他所有错误。 | 查看日志。 |

**从休眠中唤醒后。** Wi-Fi 需要几秒钟才能重新连上。笔记会等待并重试，最多 40 秒才放弃，所以刚唤醒时写的那一页可能要多等一会儿。唤醒平板的那一下按键不会让它再次休眠。如果休眠本身失败（屏幕电源在刷新后最多 30 秒内可能拒绝休眠），它会重试几次，然后重新显示页面。

**第一个字出来得很慢。** 换一个更小或更快的模型，给思考模型设置 `SAGE_REASONING=low`，或者使用离你更近的服务器。

**他不叫我的名字。** 在页面上告诉他你的名字，再查看 `facts.tsv`，看看他记下了什么。

## 14. 隐私

- **会离开平板的内容**，且只发送到你配置的模型地址：你刚写下的墨迹图片、人设、当前落座中之前几页的内容（文字形式）、你较早页面的开头几个字及日期的简短列表，以及他关于你的笔记。
- **留在平板上的内容：** 其他一切，包括你的笔迹、转写、回答、笔记和 `canon.txt`。不会上传到任何其他地方。
- 他被要求永远不要记录健康、财务、家庭住址、密码等信息，除非你明确让他记住。
- 你的密钥只保存在平板上的 `oracle.env` 中（仅 root 可读）。
- 使用本地服务器（Ollama、LM Studio）时，所有内容都不会离开你自己的网络。
