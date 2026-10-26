# Plumb — local install (pilot)

Plumb turns a requirements document into a checked, testable specification. This package runs
entirely on your computer: a single program (`bin/plumb`) with its web interface (`ui/`). It
only accepts connections from this computer (127.0.0.1).

## Before you start

- An Anthropic API key in a file, readable only by you:

  ```bash
  mkdir -p ~/.config/plumb && chmod 700 ~/.config/plumb
  # put the key (one line, starting sk-ant-api03-) into this file:
  nano ~/.config/plumb/anthropic-key && chmod 600 ~/.config/plumb/anthropic-key
  ```

  Another location works too: `export PLUMB_ANTHROPIC_KEY_FILE=/path/to/key`. Every inference
  stage calls the Anthropic API and is billed to that key.

## 1. Start Plumb for a project

```bash
./start.sh ~/plumb-projects/hr-leave
```

The first start creates the folder, its configuration (`plumb.json`, a copy of
`examples/hr-pilot.json`) and its database (`plumb.sqlite`), then opens
http://127.0.0.1:8080. Stop it with Ctrl+C. Each folder is one independent project.

The example configuration describes the HR leave pilot: its actors, roles, calendar and
expert-question routing. For another document, edit the `directory`, `calendars` and
`question_routing` sections first.

## 2. Import a document and run the steps (in the browser)

On the **Overview**, drop your document (`.md`, `.txt` or `.docx`; try
`examples/hr-leave-requirements.md`) and press **Run next step**. The Overview shows the nine
steps in order and always offers the next one. Steps that need the AI are offered only when the
API key is present.

## 3. Review in the browser

Steps never change the model on their own. Everything a step finds waits in the **Inbox**:
accept, reject or answer each item, or use **Accept all from <step>** (keyboard: J/K to move,
A to accept, R to reject). Expert questions are answered in the Inbox too. Then look at
**Requirements**, **Model**, **Scenarios** and **Quality**; when every gate passes, the Overview
offers **Export**.

The same steps also run from a terminal, with `start.sh` still running in another window:

```bash
./plumb.sh ~/plumb-projects/hr-leave import examples/hr-leave-requirements.md
./plumb.sh ~/plumb-projects/hr-leave extract
./plumb.sh ~/plumb-projects/hr-leave proposals        # list pending proposals
./plumb.sh ~/plumb-projects/hr-leave accept-set <proposal-id>=<action> ... --reason "Reviewed"
```

The step order is `extract`, `consolidate`, `lint`, `directory`, `analyse`, `flows`, then
`calc`, `rules`, `scenarios`, `tags`, `run`, `ask`, `ready` and `report --out <folder>`.
`./plumb.sh <folder> help` lists every command.

## Notes

- The model's answers vary between runs; every proposal stays reviewable and nothing is
  accepted without you.
- Very large documents (thousands of requirements) are slow in this pilot build.
- Your projects never leave this computer except for the text sent to the Anthropic API during
  inference stages.
