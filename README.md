# IronGraph 🏗️

> **The Code-as-Graph AI Operating System.**
> *Stop letting Agents hallucinate. Start making them compile.*

IronGraph is a revolutionary architecture for "Agentic Coding" that replaces the "Junior Developer" model (AI writing files blindly) with a **Strict Graph Model** (AI implementing Architected Nodes).

## 🚀 The Core Philosophy

### 1. Code-as-Graph, Not Code-as-Text
Traditional Agents see a codebase as a folder of text files. They get lost, create circular dependencies, and import things that don't exist.

**IronGraph** sees a codebase as a **Directed Acyclic Graph (DAG)** of **Capabilities**:
* **Nodes:** Vertical Slices of logic (Rust Crates).
* **Edges:** Explicit permissions (e.g., "The `Auth` node can access the `DB` node, but the `UI` node can ONLY access the `Auth` node").

### 2. The Compiler is the Boss
We use **Rust** (Backend) and **TypeScript** (Frontend) connected by **Specta**.
* **Zero-Hallucination Bridge:** The API contract (`bindings.ts`) is auto-generated from Rust types.
* **Immediate Feedback:** If the AI changes the Backend Logic but forgets to update the Frontend, the *build fails immediately*. The Agent is forced to fix it before you ever see the code.

### 3. Pure XML Protocol
We reject opaque "Function Calling" APIs. IronGraph uses a transparent **XML Protocol**:
```xml
<tool_code>
    <tool name="run_command">
        <program>cargo</program>
        <args>check</args>
    </tool>
</tool_code>
```
This allows the Agent to "Chain of Thought" and "Act" in a single, human-readable stream.

---

## 🛠️ System Architecture

### The "IronGraph Studio" App
We practice "Inception": The tool we use to build apps is, itself, an IronGraph App.

* **The Brain (`crates/llm_gateway`):**
    * Handles connection to LLMs (OpenAI/Anthropic).
    * Parses the XML Tool Protocol using `quick-xml`.
* **The Hands (`crates/workspace_manager`):**
    * Reads/Writes files with strict Sandboxing (cannot touch files outside the project).
    * Lists files recursively with intelligent filtering.
* **The Tools (`crates/terminal_manager`):**
    * Executes shell commands (`cargo`, `git`, `npm`).
    * Runs in a controlled environment rooted to the workspace.
* **The Nervous System (`useAgentLoop`):**
    * A recursive React hook that connects the Brain to the Tools, allowing autonomous execution loops.

---

## ⚡ Quick Start

### Prerequisites
* Rust (Latest Stable)
* Node.js (v18+) & pnpm
* Tauri Prerequisites (Linux: `webkit2gtk-4.1`)

### Setup
```bash
# 1. Clone the repo
git clone [https://github.com/your/irongraph.git](https://github.com/your/irongraph.git)
cd irongraph

# 2. Install Dependencies
cd apps/desktop
pnpm install

# 3. Run the Studio
pnpm tauri dev
```
