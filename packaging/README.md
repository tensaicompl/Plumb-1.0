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

## 2. Import a document and run the stages

Leave `start.sh` running and use a second terminal window. `plumb.sh` runs one Plumb command
for a project:

```bash
./plumb.sh ~/plumb-projects/hr-leave import examples/hr-leave-requirements.md
./plumb.sh ~/plumb-projects/hr-leave extract
```

Run the stages in this order. After each one, review and accept its proposals (step 3) before
running the next.

| Order | Command | What it does |
|---|---|---|
| 1 | `extract` | finds the requirements and classifies them |
| 2 | `consolidate` | builds the glossary and suggests EARS rewordings |
| 3 | `lint` | checks requirement quality (gate F1) |
| 4 | `directory` | adds the configured actors, roles, calendar and rules |
| 5 | `analyse` | entities, attributes, states and invariants (gate F2) |
| 6 | `flows` | operations and events (run `analyse` and `flows` again once they are accepted) |
| 7 | `calc`, `rules`, `scenarios`, `tags`, `run` | calculations, decision tables, test scenarios and their simulation (gate F4) |
| 8 | `ask` | turns open ambiguities into expert questions |
| 9 | `ready` | evaluates all gates |
| 10 | `report --out <folder>` | writes the specification files |

`./plumb.sh <folder> help` lists every command.

## 3. Review in the browser

Stages never change the model on their own. Everything a stage finds is a *proposal* you
accept, reject or discard on the **Proposals** screen. Then review:
- the requirements, glossary and model screens;
- the questions, which you answer on the **Questions** screen;
- the gates, which show whether the specification is complete.

To accept everything a stage proposed from the terminal instead:

```bash
./plumb.sh ~/plumb-projects/hr-leave proposals        # list pending proposals
./plumb.sh ~/plumb-projects/hr-leave accept-set <proposal-id>=<action> ... --reason "Reviewed"
```

## Notes

- The model's answers vary between runs; every proposal stays reviewable and nothing is
  accepted without you.
- Very large documents (thousands of requirements) are slow in this pilot build.
- Your projects never leave this computer except for the text sent to the Anthropic API during
  inference stages.
