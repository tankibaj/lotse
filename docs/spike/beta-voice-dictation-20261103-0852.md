# Spike: macOS Dictation in Lotse's WKWebView (xterm.js and the composer)
**Status:** Blocked | **Owner:** harnex | **Updated:** 2026-11-03 08:52
Blocked: the experiment needs a person at the Mac (Dictation on, a spoken phrase, the built app). The implementer ran headless and could not do it. Nothing below is a measured result.

## Question
Does macOS Dictation, started with the user's own shortcut (never hard-coded by Lotse), type into (a) the xterm.js terminal and (b) the composer `<textarea>` of the built app? The answer gates wp19 (voice-mic, `objc2-speech` + `objc2-avf-audio`).

## Ready for the run
- Built app: `target/release/bundle/macos/Lotse.app` (wp03 branch, `npx tauri build --bundles app`; not yet ad-hoc signed by `scripts/build-app.sh`, which the headless run could not start). `Info.plist` in the bundle carries `NSMicrophoneUsageDescription`, `NSSpeechRecognitionUsageDescription`, `NSUserNotificationAlertStyle=alert` (checked in the file).
- The composer is a plain `<textarea aria-label="Prompt composer">`, opened with ⌘⇧Return (`openComposer`), Return sends, ⇧Return adds a line, Esc closes.
- System Settings › Keyboard › Dictation: on, with its shortcut left as the user has it.

## Procedure (about 15 min)
Start with `open target/release/bundle/macos/Lotse.app --args --isolated-data-dir "$(mktemp -d)"` so the real data stays untouched. Start a plain Terminal session (no agent).
1. Click into the terminal, start Dictation with the user's shortcut, say "echo hello world". Record: the text appears at the prompt? As one insert or word by word? Corrections replace earlier text?
2. Same in a Claude Code session prompt (an agent TUI in raw mode).
3. Open the composer (⌘⇧Return), start Dictation, say two sentences. Record: text in the textarea? Return after Dictation stops sends exactly the dictated text plus `\r`?
4. First use: does the microphone / speech prompt appear and name the Info.plist text? Is it needed for system Dictation at all (expected no: Dictation runs in the system's own process)?
5. Note any difference between light/dark, and with the sidebar hidden.

## Results
| Case | Result | Notes |
|---|---|---|
| xterm.js (shell prompt) | NOT RUN | |
| xterm.js (agent TUI) | NOT RUN | |
| Composer textarea | NOT RUN | |
| Permission prompt text | NOT RUN | |

## Hypotheses (unverified, from how the pieces are built)
- The composer should work: Dictation is a system text-input service and a `<textarea>` in WKWebView is a normal text input client.
- xterm.js is the unknown: it reads input through its own hidden helper textarea and turns `input` / composition events into PTY data. Dictation's replace-as-you-speak edits (it rewrites earlier words) may arrive as replacement edits xterm does not map, so partial or duplicated text is plausible.

## Decision for wp19
Not made. Suggested rule once the run is done: composer works → wp19 is optional polish (a mic button that starts the user's own Dictation is not possible without hard-coding a shortcut, so wp19's own `objc2-speech` path is what would add value); composer fails → wp19 is needed, and so is a fix to the composer first. The orchestrator or Naim decides.
