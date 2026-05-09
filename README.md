# Loomiq 🔨

Loomiq is an npm-style prompt package manager and AI pipeline execution engine designed for developers. Originally prototyped in TypeScript, it has been **completely rewritten in Rust** to provide a blazing fast, single-binary, natively compiled execution environment.

It simplifies the process of chaining prompts, passing data between steps, caching responses to save tokens and time, and enforcing structured JSON outputs.

## 🚀 Features

- **NPM-style Packages**: Install prompt packages from a local registry. Dependencies are flattened and versioned automatically (e.g., `~/.loomiq/modules/pkg@1.0.0`).
- **Pipeline Execution**: Chain prompts sequentially, passing outputs from one step to inputs of the next using a simple `{{step.output}}` DSL.
- **Provider Agnostic**: Use multiple LLM providers (currently supports `openai` and `mock` for testing).
- **Smart Caching**: Step-level caching based on a hash of the template, inputs, config, and model to avoid redundant LLM calls.
- **Strict Validation**: Fails fast with clear error messages if required inputs are missing or misspelled.
- **Native Performance**: Built in Rust for maximum speed, memory safety, and cross-platform native binaries.

## 📦 Architecture

The project is structured as a Rust Cargo Workspace:

- `crates/core`: Types, Serde YAML serialization/deserialization, and `thiserror` error handling.
- `crates/engine`: Core pipeline executor, LLM async traits (via `reqwest`), template resolving (via `regex`), and the SHA-256 caching system.
- `crates/cli`: Command-line tool built with `clap` and `anyhow` for package management and pipeline execution.
- `/registry`: A local directory containing ready-to-use MVP packages acting as the package source.

## 🛠️ Usage

Ensure you have Rust installed. You can interact with the CLI directly using Cargo during development.

### 1. Install a Prompt Package
Install a package from the local `/registry` to your local `~/.loomiq/modules` directory. Dependencies are installed recursively.
```bash
loomiq install youtube-growth-kit
```

### 2. Run a Pipeline or Prompt
Execute an installed package. You must provide all inputs defined in the package's manifest.
```bash
# Using the mock provider (default)
loomiq run youtube-growth-kit --input niche="AI tools" --provider mock

# Run a single standalone prompt
loomiq run topic-generator --input industry="cooking"

# Using OpenAI (requires OPENAI_API_KEY environment variable)
loomiq run youtube-growth-kit --input niche="AI tools" --provider openai
```

### 3. Compile to Single Prompt
If you want to run the pipeline manually (e.g., paste it into an external LLM interface), you can compile the entire pipeline into a single unified prompt string:
```bash
loomiq compile youtube-growth-kit --input niche="AI tools"
```

### 4. Manage Cache & Packages
Loomiq caches outputs in `.loomiq/cache`. 
```bash
# Clear the cache
loomiq cache clear

# List installed packages
loomiq list
```

## 🧩 MVP Packages

The repository comes with two built-in pipelines in the `/registry` (plus their standalone prompt dependencies):

1. **`youtube-growth-kit`**: Generates a topic idea -> drafts a video script -> creates viral titles.
2. **`freelance-client-machine`**: Finds a unique service angle -> writes a cold DM -> drafts a follow-up proposal.

## 🐙 CI/CD & Releases
Loomiq leverages GitHub Actions for Continuous Integration (`fmt`, `clippy`, `test`) and Continuous Deployment. Pushing a tag (e.g., `v1.0.0`) automatically compiles native binaries for Linux, macOS (Apple Silicon & Intel), and Windows, and uploads them to GitHub Releases.

## 🔮 Future Improvements

- **DAG Pipeline Engine**: Upgrade the sequential executor to support Directed Acyclic Graphs via `tokio` for parallel async step execution.
- **Remote Registry**: Implement a real backend for publishing and downloading packages via HTTP (`loomiq install @user/package`).
- **More Providers**: Add native support for Anthropic, Google Gemini, and local models (via Ollama).
