# Security Policy

## Reporting a vulnerability

Please **do not open a public issue**. Use GitHub's private reporting instead:
**Security → Report a vulnerability** on this repository. It reaches the
maintainer directly and stays private until a fix ships.

Tell us what you can: what the flaw is, how to reproduce it, and what an
attacker gets out of it. A proof of concept helps, but a clear description is
already plenty.

Expect a first reply within a week. This is a small project maintained by one
person, so please be patient with the timeline — not with the risk.

## Supported versions

Only the latest release is supported. Fixes ship in a new version rather than
as patches to older ones.

## What is in scope

KaraokeTyper transcribes locally and sends nothing to a server, so the surface
is mostly local:

- **Model downloads.** Files are fetched over HTTPS and verified against a
  SHA-256 pinned in the catalogue. A path that installs an unverified file, or
  one that escapes the models directory, is a vulnerability.
- **Text injection.** The transcript is typed into whatever window has focus,
  through `xdotool`, `enigo`, `uinput` or the clipboard. Anything that sends it
  somewhere else, or leaves it in the clipboard, counts.
- **Input capture.** The push-to-talk listener reads `evdev` on Linux. Anything
  that logs or forwards keystrokes beyond the shortcut itself counts.
- **The Ollama integration**, when enabled: it posts the transcript to a URL
  from the settings. Anything that sends it to an unintended host counts.

Out of scope: the models themselves, their transcription quality, and anything
requiring an attacker who already has your user account.
