# Circuit Breaker Interactive TUI

A full interactive terminal interface for Circuit Breaker with slash commands.

## Quick Start

```bash
# Just run cb with no arguments to launch the interactive TUI
./cb
```

## Screenshot

```
╭──────────────────────────────────────────────────────────────────────────╮
│ Circuit Breaker                              NATS: ● connected (47 events)│
╰──────────────────────────────────────────────────────────────────────────╯

Run: ci-pipeline (abc123) [running]

┌──────────────────────────────────────────────────────────────────────────┐
│ [10:23:15] Welcome to Circuit Breaker TUI                                │
│ [10:23:15] Type /help for available commands                             │
│ [10:23:15] Connected to NATS at nats://localhost:4222                    │
│ [10:23:20] /run examples/ci-pipeline/workflow.ts                         │
│ [10:23:20] Loaded workflow: ci-pipeline                                  │
│ [10:23:20] Validation passed                                             │
│ [10:23:21] Submitted: wf-abc123                                          │
│ [10:23:21] Started run: run-xyz789                                       │
│ [10:23:21] Monitoring run run-xyz789 - events will appear below          │
│ [10:23:22] Transition enabled: checkout                                  │
│ [10:23:22] Transition started: checkout                                  │
│ [10:23:23] Transition completed: checkout - 1234ms                       │
│ [10:23:23] Transition enabled: build                                     │
│ [10:23:23] Transition started: build                                     │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘

❯ _

/help for commands | /run to start a workflow | /quit to exit
```

## Slash Commands

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/run <workflow>` | Run a workflow file (`.ts`, `.js`, or `.json`) |
| `/status [run-id]` | Get status of current or specified run |
| `/logs [run-id]` | View logs for current or specified run |
| `/list` | List recent workflow runs |
| `/workflows` | List available workflows |
| `/inject <place> [data]` | Inject a token into a place |
| `/describe [run-id]` | Describe workflow structure |
| `/cancel [run-id]` | Cancel a running workflow |
| `/clear` | Clear the log output |
| `/connect` | Reconnect to NATS |
| `/quit` | Exit the TUI |

## Features

### Event-Driven Updates
The TUI subscribes to NATS event streams for real-time updates:
- No polling - instant updates when events occur
- See transition states change as they happen
- Live event counter shows connection health

### Current Run Tracking
When you run a workflow, the TUI automatically:
- Tracks the run as "current"
- Filters events to show only relevant ones
- Displays status in the header
- Auto-updates status from NATS events

### Command History
- Use **Up/Down arrows** to navigate command history
- History persists during the session

### Log Types
Logs are color-coded by type:
- **Blue**: Info messages
- **Green**: Success messages
- **Red**: Error messages
- **Yellow**: Warnings
- **Cyan**: NATS events
- **Magenta**: Commands entered

## Examples

### Run a Workflow

```
❯ /run examples/hello-world/workflow.ts
[10:30:00] /run examples/hello-world/workflow.ts
[10:30:00] Loaded workflow: hello-world
[10:30:00] Validation passed
[10:30:01] Submitted: wf-abc123
[10:30:01] Started run: run-xyz789
[10:30:01] Monitoring run run-xyz789 - events will appear below
[10:30:02] Transition enabled: greet
[10:30:02] Transition started: greet
[10:30:03] Transition completed: greet - 1000ms
[10:30:03] Run status: completed
```

### Check Status

```
❯ /status
[10:31:00] /status
[10:31:00] ─── Run run-xyz789 ───
[10:31:00]   Workflow: hello-world
[10:31:00]   Status:   completed
[10:31:00]   Started:  2024-01-15T10:30:01.000Z
[10:31:00]   Completed: 2024-01-15T10:30:03.000Z
```

### List Runs

```
❯ /list
[10:32:00] /list
[10:32:00] ─── Recent Runs ───
[10:32:00]   ✓ abc12345 - hello-world (completed)
[10:32:00]   ✓ def67890 - ci-pipeline (completed)
[10:32:00]   ✗ ghi11111 - deploy-prod (failed)
[10:32:00]   ⟳ jkl22222 - data-pipeline (running)
```

### Inject a Token

```
❯ /inject ready-for-deploy '{"environment": "staging"}'
[10:33:00] /inject ready-for-deploy '{"environment": "staging"}'
[10:33:00] Token injected into ready-for-deploy
[10:33:01] Token injected into: ready-for-deploy
[10:33:01] Transition enabled: deploy
```

### Describe Workflow

```
❯ /describe
[10:34:00] /describe
[10:34:00] ─── Run run-xyz789 ───
[10:34:00] Places:
[10:34:00]   ○ start
[10:34:00]   ○ built
[10:34:00]   ● done
[10:34:00] Transitions:
[10:34:00]   completed - build
[10:34:00]   completed - test
```

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    TUI (Ink React)                   │
│                                                      │
│  ┌─────────────────────────────────────────────┐   │
│  │           Command Input (TextInput)          │   │
│  └─────────────────────────────────────────────┘   │
│                        │                            │
│                        ▼                            │
│  ┌─────────────────────────────────────────────┐   │
│  │          Command Parser & Executor           │   │
│  │                                              │   │
│  │  /run → runWorkflow()                       │   │
│  │  /status → showStatus()                     │   │
│  │  /inject → injectToken()                    │   │
│  │  ...                                        │   │
│  └─────────────────────────────────────────────┘   │
│                        │                            │
│            ┌───────────┴───────────┐               │
│            ▼                       ▼               │
│     ┌─────────────┐         ┌─────────────┐       │
│     │  REST API   │         │    NATS     │       │
│     │  (commands) │         │  (events)   │       │
│     └─────────────┘         └─────────────┘       │
│                                    │               │
│                                    ▼               │
│     ┌─────────────────────────────────────────┐   │
│     │            Event Handler                 │   │
│     │                                          │   │
│     │  cb.runs.*.status → Update status       │   │
│     │  cb.runs.*.transitions.*.* → Log event  │   │
│     │  cb.runs.*.tokens.*.injected → Log      │   │
│     └─────────────────────────────────────────┘   │
│                        │                           │
│                        ▼                           │
│     ┌─────────────────────────────────────────┐   │
│     │              Log Output                  │   │
│     │         (Scrollable, colored)            │   │
│     └─────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `NATS_URL` | NATS server URL | `nats://localhost:4222` |
| `CIRCUIT_BREAKER_API` | API server URL | `http://localhost:8080` |
| `CIRCUIT_BREAKER_API_KEY` | API key (if required) | - |

## Technical Details

### Built With
- **Ink**: React for CLIs
- **React 19**: Component rendering
- **ink-text-input**: Command input field
- **ink-spinner**: Loading indicators
- **NATS.js**: Event subscriptions

### State Management
- React `useState` for component state
- `useRef` for NATS connection persistence
- `useInput` for keyboard handling
- `useEffect` for initialization and cleanup

### Event Flow
1. User enters command → `executeCommand()`
2. Command calls API or manipulates state
3. NATS events arrive → `handleNatsEvent()`
4. State updates → React re-renders
5. Log output updates with new entries

## Comparison to Traditional CLI

| Aspect | Traditional CLI | Interactive TUI |
|--------|-----------------|-----------------|
| Invocation | `./cb run workflow.ts` | `./cb` then `/run workflow.ts` |
| Monitoring | Separate `./cb status` calls | Real-time in same session |
| Events | Poll with `--watch` | Instant via NATS subscription |
| Context | Stateless | Tracks current run |
| History | Shell history | In-app command history |

## Troubleshooting

### TUI won't start
```bash
# Check terminal supports raw mode
echo $TERM  # Should be xterm-256color or similar

# Try with explicit terminal
TERM=xterm-256color ./cb
```

### NATS connection failed
```bash
# Check NATS is running
docker ps | grep nats

# Try explicit URL
NATS_URL="nats://localhost:4222" ./cb
```

### Commands not working
```bash
# Check API is reachable
./cb health

# Check API URL
CIRCUIT_BREAKER_API="http://localhost:8080" ./cb
```

## Future Enhancements

- [ ] Tab completion for commands
- [ ] Workflow file path completion
- [ ] Multiple run monitoring (split view)
- [ ] Bookmark/favorite workflows
- [ ] Custom themes/colors
- [ ] Plugin system for custom commands
- [ ] Session persistence
- [ ] Export logs to file
