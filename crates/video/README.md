# Amarcode — One continuous conversation

A **30-second, 1920 × 1080, 60 fps** product walkthrough made with Remotion.

The story follows Amarcode's core promise: **alternate among several agents using the subscriptions you already have, without extra API keys, while carrying the conversation forward.** The demonstration uses Codex with a ChatGPT subscription and Claude with a Claude subscription. Amarcode keeps the visible conversation independently of each agent's native session.

The application stays on screen for the entire film. It is one continuous Playwright recording: real mouse movement, real typing, streaming messages, two agent switches, the actual file tree and diff viewer, and returning to saved history. There are no feature slides or opening/closing title cards.

## Watch and edit

- `out/amarcode-launch.mp4` — final film, including stereo music and interaction sounds.
- `out/poster.png` — thumbnail.
- `out/contact-sheet.jpg` — overview extracted from the encoded film.
- `public/walkthrough/app-walkthrough.mp4` — uninterrupted application footage.
- `out/previous/first-cut.mp4` — previous version, retained locally for comparison.

Outputs under `out/` are ignored by Git.

```sh
# From the repository root:
bun install
cd crates/video
bun run studio
bun run render
bun run verify
bun run contact-sheet
```

Remotion can download its own Chrome Headless Shell. To use an existing browser, set `CHROME_PATH=/path/to/chrome` before render or capture. FFmpeg and FFprobe are required for capture processing, finalization, and verification.

The composition is `AmarcodeLaunch` in `src/index.tsx`, with 1,800 frames. `src/LaunchFilm.tsx` controls the camera, subscription indicators, and walkthrough captions. A single `OffthreadVideo` stays mounted for all 30 seconds; there are no screenshot swaps.

## Walkthrough

| Time     | Actual interaction                                                | What it illustrates                                           |
| -------- | ----------------------------------------------------------------- | ------------------------------------------------------------- |
| 0–4s     | Open the agent picker, choose Codex, type a request               | Use a subscription-authenticated agent without extra API keys |
| 4–7s     | Codex streams its first answer                                    | Start work in one conversation                                |
| 7–10s    | Choose Claude in the same composer; type “Add a subtle entrance.” | Switch agents mid-conversation without restating the project  |
| 10–13s   | Claude continues the work Codex started                           | Saved conversation context follows the handoff                |
| 13–16s   | Switch back to Codex; type “Respect reduced motion, too.”         | Alternate freely between your agents                          |
| 16–19.5s | Codex builds on Claude's changes                                  | Each perspective stays in the same conversation               |
| 19.5–24s | Open source files and the real Git diff view                      | Review the work inside Amarcode                               |
| 24–27s   | Leave the chat and reopen it from the sidebar                     | Your history remains in Amarcode                              |
| 27–30s   | The same complete chat stays visible                              | Different agents. One continuous thought.                     |

## Visual direction

The framing comes from the actual landing website, rather than a separate visual identity:

- **Cormorant Garamond** 300/400, copied from `landing/public/fonts`.
- Paper `#fafaf6`, ink `#242c2b`, muted ink `#5b6561`, rule `#d5d9d0`, pale sage `#edf2ed`.
- The landing page's actual blue cloud shader, captured without page copy in `public/landing/sky.png`.
- Thin page borders, registration crosses, square app framing, quiet captions.
- The real application in its light monochrome theme, with native Roboto typography.

The app is the focus. Camera moves follow the picker and composer; captions explain the action below the recording. The subscription labels remain visible above it.

## Capture and data

```sh
# Once, if needed:
bunx playwright install chromium ffmpeg
bun run capture
# Optional: recapture the landing site cloud shader
bun run capture:landing
bun run audio
bun run render
bun run verify
```

The capture script starts the unmodified application on port 1420. A native 1920 × 1080 viewport at 150% browser zoom produces readable app text in the full 1080p / 60 fps recording. A synchronization flash is removed automatically so recording and caption timing agree. `public/walkthrough/manifest.json` records the actual click/submission times and browser errors.

`scripts/demo-bridge.mjs` supplies fictional, deterministic daemon/native responses. It stages already-authenticated agents, the Orbit project, and the generated answers. Those answers are demonstration data, not actual provider executions or a subscription billing test. All displayed controls, message rendering, navigation, picker transitions, and file/diff views are the application's real components. Production application files are untouched.

The video describes retained conversation content. It does not claim portability of hidden model state: the README explains native resumption where available and saved-transcript hydration otherwise.

The original score is regenerated by `scripts/audio.py` (Python 3 + NumPy). It uses no third-party samples. Light pointer and keyboard sounds align with the capture manifest. Fonts and their licenses are stored locally; rendering needs no remote font requests.

## Verification

`bun run typecheck` checks the Remotion composition. `bun run verify` checks the encoded video's exact 30-second duration, 1920 × 1080 resolution, 60 fps, 1,800 frames, H.264 codec, and stereo audio. The capture completes both switches and the saved-chat return without browser errors.

`render` finalizes AAC padding to preserve exactly 30 seconds and places MP4 metadata at the front for streaming playback.
