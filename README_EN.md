# EspSmith

<p align="center">
  <a href="README.md">中文</a> | <a href="README_EN.md">English</a>
</p>

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="EspSmith Logo" width="120" />
</p>

<p align="center">
  <strong>AI-Powered ESP32 Integrated Development Environment</strong>
</p>

<p align="center">
  <em>Embedding AI large models into embedded development workflow, achieving full closed-loop automation for code writing, compilation, firmware flashing, and hardware debugging</em>
</p>

***

## Project Overview

EspSmith is a modern integrated development environment (IDE) for ESP32 series chips, deeply integrating AI large models into the embedded development workflow. By integrating AI services like DeepSeek / Ollama, developers can describe requirements in natural language, and AI automatically completes the full closed-loop of code writing, IDF compilation, firmware flashing, and serial verification. For development boards supporting USB-JTAG, it also provides advanced debugging capabilities including hardware breakpoints, variable monitoring, and register analysis.

### Core Features

| Category | Feature |
| -------- | ------- |
| **AI Intelligent Programming** | Integrated DeepSeek / Ollama local models, natural language driven full closed-loop development |
| **Code Editor** | Based on Monaco Editor, supports C/C++ syntax highlighting, ESP-IDF code snippets, multi-tab management |
| **ESP-IDF Integration** | Auto-detect IDF environment, one-click compile/flash/monitor/config, supports all ESP32 series chips |
| **JTAG Hardware Debugging** | USB-JTAG auto-recognition, supports breakpoints/single-step/variable monitoring/register/call stack/CoreDump analysis |
| **Serial Monitor** | Real-time serial data transceive, supports multiple baud rates, with timestamp and log filtering |
| **Hardware Configuration** | Visual pin assignment, auto conflict detection, one-click export to C header file |
| **Git Integration** | Built-in Git panel, supports AI commit, change tracking, branch management (Coming Soon) |
| **Hot-Plug Detection** | Auto-detect device plug/unplug, smart recognition of JTAG/UART mode, no manual configuration needed |
| **Internationalization** | Supports Chinese / English bilingual interface switching |
| **Self-Healing Engine** | plan → preflight → build → flash → verify closed-loop pipeline |
| **Experience Engine** | Cross-run experience accumulation, records historical repair skills for AI reference |

***

## Software Download

Latest Version: **v0.1.0**

- 🔗 **Release Page**: [GitHub Releases](https://github.com/fangkuaiLS/EspSmith/releases)

## Screenshots
<p align="center">
  <em>Supports dual mode switching: AUTO mode and CODE mode via the top-left LOGO toggle button</em>
</p>
<p align="center">
  <img src="docs/1.png" alt="EspSmith Main Interface" width="600" />
</p>
<p align="center">
  <em>AUTO Mode - AI Chat Panel</em>
</p>

<p align="center">
  <img src="docs/2.png" alt="ESP-IDF Compile and Flash" width="600" />
</p>
<p align="center">
  <em>AUTO Mode - AI Closed-Loop Flashing</em>
</p>

<p align="center">
  <img src="docs/3.png" alt="JTAG Hardware Debugging" width="600" />
</p>
<p align="center">
  <em>CODE Mode - Provides professional code editing and debugging features</em>
</p>

***

## System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Frontend (React)                     │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐ │
│  │ FileTree │ │  Editor  │ │ Chat(AI) │ │  Settings  │ │
│  └──────────┘ └──────────┘ └──────────┘ └────────────┘ │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐ │
│  │ Hardware │ │  Build   │ │  Serial  │ │   Debug    │ │
│  └──────────┘ └──────────┘ └──────────┘ └────────────┘ │
├─────────────────────────────────────────────────────────┤
│                  Tauri Bridge (IPC)                      │
├─────────────────────────────────────────────────────────┤
│                    Backend (Rust)                        │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────────┐ │
│  │ Commands │ │   IDF    │ │   MCP    │ │    AI      │ │
│  └──────────┘ └──────────┘ └──────────┘ └────────────┘ │
│  ┌──────────┐ ┌────────────┐ ┌──────────┐ ┌──────────┐ │
│  │ Adapters │ │Self-Healing│ │Connection│ │Experience│ │
│  └──────────┘ └────────────┘ └──────────┘ └──────────┘ │
└─────────────────────────────────────────────────────────┘
```

Backend Rust Module Structure:

| Module | Path | Responsibility |
| ------ | ---- | -------------- |
| `commands/` | `src-tauri/src/commands/` | Project, file, hardware, build, flash, serial, GDB debug, Git commands |
| `idf.rs` | `src-tauri/src/idf.rs` | ESP-IDF toolchain wrapper, auto-detection, command execution, error parsing |
| `ai_assistant.rs` | `src-tauri/src/ai_assistant.rs` | DeepSeek/Ollama AI integration, CodeWhale Agent process management, Token usage statistics |
| `mcp.rs` | `src-tauri/src/mcp.rs` | MCP (Model Context Protocol) server, provides tool calling capability for AI Agent |
| `connection.rs` | `src-tauri/src/connection.rs` | USB-JTAG/UART auto-detection, chip identification, connection mode management |
| `self_healing/` | `src-tauri/src/self_healing/` | Closed-loop self-healing engine (plan → preflight → build → flash → verify) |
| `adapters/` | `src-tauri/src/adapters/` | Adapter pattern abstraction layer, supports IDF/esptool/OpenOCD/GDB and other tools |
| `instruments/` | `src-tauri/src/instruments/` | Instrument abstraction (JTAG/ST-Link/DAP-Link), health check registry |
| `experience/` | `src-tauri/src/experience/` | Cross-run experience accumulation engine, records repair skills and known pitfalls |

***

## Core Engines

EspSmith has two forward-looking core engines that solve the **reliability** and **evolvability** problems in embedded development.

### Self-Healing Engine — Closed-Loop Reliability Engine

In embedded development, a complete verification closed-loop requires multiple steps: **compile → flash → serial verification**. Failure at any step may interrupt the entire process. The Self-Healing engine formalizes this process as a **state machine with automatic recovery**.

```
plan → preflight → build → flash → verify → report
  ↑                    ↑        ↑        ↑
  └── Any step fails ── retry ── recover ── rollback anchor
```

**Core Capabilities:**

| Capability | Description |
| ---------- | ----------- |
| **Step Orchestration** | Defines build / flash / verify as ordered steps, each bound to an adapter, supports IDF, esptool, OpenOCD, GDB and other toolchains |
| **Tiered Retry** | Allocates independent retry budget by step type (Build: 1x, Load: 2x, Check: 2x), avoids wasting resources on invalid retries |
| **Intelligent Recovery** | Automatically analyzes error type on failure (compile error / flash failure / serial timeout / OpenOCD exception), matches the most suitable recovery action |
| **Anchor Rollback** | After recovery, automatically rolls back to the correct anchor (Build / Load / Check), not starting from scratch, saves time |
| **Safety Guardrails** | Dual protection: total execution count limit (guard_limit) + global timeout (timeout_s), prevents infinite loops |
| **Hardware Recovery** | 4 recovery actions: DTR/RTS serial reset → OpenOCD soft reset → OpenOCD hard reset → manual power cycle, auto-escalating intensity |
| **GDB Session Persistence** | Auto-reconnect GDB session after probe reset, breakpoints and watch states preserved |

**Recovery Strategy Example:**

```
flash step fails "OpenOCD connection refused"
  → Classify: OpenOCD/probe error → Anchor: Load
  → Action: ProbeHardReset (send reset via OpenOCD telnet)
  → Rollback to flash step and retry
  → Auto-reconnect GDB session
```

### Experience Engine — Cross-Run Experience Accumulation Engine

Traditional IDEs start from scratch each time and don't learn from history. The Experience engine makes EspSmith an **evolving development environment** — it records the results of each build/flash/verification, extracts reusable engineering experience, and injects it into the AI's context.

```
┌─────────────────────────────────────────────────┐
│               Experience Engine                   │
│                                                   │
│  ┌──────────┐  ┌──────────┐  ┌────────────────┐  │
│  │ RunStats │  │  Skills  │  │   Pitfalls     │  │
│  │          │  │          │  │                │  │
│  │ Success/ │  │ Trigger→ │  │ Historical     │  │
│  │ Failure  │  │   Fix    │  │ failure modes  │  │
│  │Confidence│  │  Lesson  │  │ Dangerous ops  │  │
│  └──────────┘  └──────────┘  └────────────────┘  │
│                                                   │
│  ┌──────────────────────────────────────────────┐ │
│  │        AI Context Prompt Injection           │ │
│  │  "Based on historical experience, this chip's│ │
│  │   JTAG is unstable at 40MHz, suggest 20MHz..."│ │
│  └──────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

**Core Capabilities:**

| Capability | Description |
| ---------- | ----------- |
| **Run Statistics** | Automatically tracks total runs, success/failure count, confidence (0-100%) for each `board:test` pair, visualizes project stability |
| **Skill Recording** | Structured storage of engineering experience: `trigger` (trigger condition) → `fix` (solution) → `lesson` (experience learned), supports scope filtering (global/by chip/by project) |
| **Pitfall Recognition** | Automatically extracts "known pitfalls" and "focus points" from historical failures, proactively reminds before next run |
| **AI Context Injection** | Automatically generates AI system prompt from accumulated experience, lets LLM avoid known pitfalls when generating code |
| **Persistent Storage** | JSON-based file storage (`<project>/.espsmith/experience/`), human-readable, easy for version control and sharing |
| **Cross-Project Reuse** | Scope mechanism supports global experience (`all`), chip-level experience (`esp32s3`), project-level experience, flexible control of sharing scope |

### Dual Engine Synergy

Self-Healing and Experience don't run independently, but form a **positive feedback loop**:

```
Self-Healing executes → fails → auto-recovery → records result
                                    ↓
                            Experience accumulates
                                    ↓
                            Next AI code generation → proactively avoids known pitfalls
                                    ↓
                            Self-Healing success rate ↑ → confidence ↑
```

This design makes EspSmith not just an IDE, but a **embedded development partner that gets smarter with use**.

***

### AI Closed-Loop Development Flow

```
User inputs natural language requirement
        │
        ▼
┌──────────────────┐
│  1. AI Understands│  Analyzes project code structure, understands hardware config
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│  2. Generate/Modify│  write_file writes source files
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│  3. Compile Build │  espsmith.exe build → returns compile errors (if any)
└──────┬───────────┘
       │ Compile failed → return to step 2 to fix
       ▼
┌──────────────────┐
│  4. Firmware Flash│  JTAG: closed_loop one-click flash
│                  │  UART: esptool flash
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│  5. Serial Verify │  Read serial output, verify functionality correctness
└──────┬───────────┘
       │ Exception → trigger GDB debug
       ▼
┌──────────────────┐
│  6. JTAG Deep Debug│  Hardware breakpoints, variable monitoring, register analysis, call stack tracing
└──────┬───────────┘
       │
       ▼
┌──────────────────┐
│  7. Report Result │  Report all operation results in Chinese, record experience to Experience engine
└──────────────────┘
```

### JTAG vs UART Mode

| Feature | JTAG Mode | UART Mode |
| ------- | --------- | --------- |
| Supported Chips | ESP32-S3/C3/C6/H2/P4 | All ESP32 series |
| Hardware Breakpoints | ✅ Supported | ❌ Not supported |
| Variable Monitoring | ✅ Supported | ❌ Not supported |
| Register View | ✅ Supported | ❌ Not supported |
| Call Stack Analysis | ✅ Supported | ❌ Not supported |
| CoreDump Analysis | ✅ Supported | ❌ Not supported |
| Firmware Flash | ✅ OpenOCD | ✅ esptool |
| Auto Detection | ✅ Auto switch | ✅ Auto switch |

***

## Deployment Guide

### Requirements

| Dependency | Version | Description |
| ---------- | ------- | ----------- |
| **Node.js** | ≥ 18 | Frontend build toolchain |
| **Rust** | ≥ 1.77 | Tauri backend compilation (1.77+ includes UTF-8 process fix, avoids Chinese Windows compile errors) |
| **ESP-IDF** | v5.0+ | ESP32 development framework (optional but recommended) |
| **CodeWhale** | latest | AI Agent CLI (required for AI features) |

### Installation Steps

#### 1. Clone Project

```bash
git clone <repository-url>
cd espsmith
```

#### 2. Install Frontend Dependencies

```bash
npm install
```

#### 3. Start Development Mode

```bash
# Frontend dev server + Tauri desktop window
npm run tauri -- dev

# Or start frontend only (browser mode debugging)
npm run dev
```

#### 4. Build Production Package

```bash
npm run tauri -- build
```

Build artifacts located at `src-tauri/target/release/bundle/`.

> **💡 Chinese Windows Users**: The startup script automatically detects environment and redirects Chinese paths (like user directory names) to ASCII paths within the project, avoiding Rust build script compilation errors. No extra configuration needed.

### AI Feature Configuration

EspSmith connects to AI services through CodeWhale CLI Agent.

#### Configure API Key

In EspSmith settings panel, fill in:

- **AI Provider**: DeepSeek or Ollama
- **API Key**: DeepSeek API Key (get from [platform.deepseek.com](https://platform.deepseek.com))
- **ESP-IDF Path**: ESP-IDF installation directory (e.g., E:.espressif\v5.5.4\esp-idf)

#### Using Ollama Local Model

```bash
# Install Ollama
# Download: https://ollama.com

# Pull model
ollama pull qwen2.5-coder:7b

# Select Ollama as AI Provider in EspSmith settings
```

### ESP-IDF Deployment

EspSmith is compatible with multiple ESP-IDF installation methods:

1. **EIM (ESP-IDF Install Manager)** — Auto-detects `%USERPROFILE%\.espressif\eim_idf.json`
2. **VSCode Extension Install** — Auto-detects VSCode ESP-IDF extension installation path
3. **Manual Install** — Manually specify IDF path in settings
4. **Environment Variable** — Auto-recognizes `IDF_PATH` environment variable

### JTAG Debug Configuration (OpenOCD)

Before using JTAG debugging, need to configure OpenOCD environment variables. EspSmith searches for OpenOCD in this **priority**:

1. `OPENOCD_BIN` environment variable (recommended, most reliable)
2. `IDF_PATH` / `tools/openocd/openocd.exe`
3. `~/.espressif/tools/openocd-esp32/bin/openocd.exe`
4. `openocd` in system PATH

#### Verify OpenOCD Configuration

```bash
# Check if OpenOCD is available
openocd --version
```

#### JTAG Supported Chips

| Chip | USB-JTAG Support | Debug Interface |
| ---- | ---------------- | --------------- |
| ESP32-S3 | ✅ | esp_usb_jtag |
| ESP32-C3 | ✅ | esp_usb_jtag |
| ESP32-C5 | ✅ | esp_usb_jtag |
| ESP32-C6 | ✅ | esp_usb_jtag |
| ESP32-C61 | ✅ | esp_usb_jtag |
| ESP32-H2 | ✅ | esp_usb_jtag |
| ESP32-P4 | ✅ | esp_usb_jtag |
| ESP32 | ⚠️ External JTAG needed | ftdi/esp32_devkitj_v1 |
| ESP32-S2 | ⚠️ External JTAG needed | ftdi/esp32_devkitj_v1 |

***

## Inspiration

This project draws inspiration from the following excellent open-source projects:

- **[AEL (AI Embedded Lab)](https://github.com/nicekwell/AI-Instrument-Closed-Loop)** — Multi-instrument closed-loop debugging system, inspired the Self-Healing engine and Experience engine design
- **[VS Code ESP-IDF Extension](https://github.com/espressif/vscode-esp-idf-extension)** — Official ESP-IDF extension, inspired IDF workflow and serial management
- **[CodeWhale](https://github.com/anthropics/codewhale)** — AI Agent CLI tool, inspired MCP protocol integration and AI assistant module design

***

## License

MIT License - See [LICENSE](LICENSE) for details.

***

<p align="center">
  <strong>Made with ❤️ for ESP32 Developers</strong>
</p>
