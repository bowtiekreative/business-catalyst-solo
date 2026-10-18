# Catalyst Solo — handoff package

This package contains a clickable prototype and a production brief for Catalyst Solo, a desktop app for managing one existing website.

- `PRODUCTION_PROMPT_SOLO.md`: the production brief. Paste it into your coding agent.
- `prototype/`: the clickable prototype.
  - Serve this folder and open `Catalyst Solo.dc.html`, e.g. `npx serve prototype`.
  - The page loads its section library over HTTP, so opening it directly as a local file won't work.
- `skills/catalyst-solo-build/SKILL.md`: the build skill.
  - **Claude Code:** copy the `catalyst-solo-build` folder into `.claude/skills/` in your new repo, or into `~/.claude/skills/` to use it everywhere.
  - **Other agents:** paste `SKILL.md` into the system prompt.

## Quick start with Claude Code

1. Create a new repository and copy this whole folder into it as `handoff/`.
2. Copy `handoff/skills/catalyst-solo-build` into `.claude/skills/`.
3. Ask: "Use the catalyst-solo-build skill. Read handoff/PRODUCTION_PROMPT_SOLO.md and handoff/prototype, then start milestone 1."

## What's simulated in the prototype

- AI proposals come from built-in rules (Claude is used only when it's available in the browser).
- Git, publishing and the folder picker are simulated.
- Tablet and phone preview show the desktop layout scaled down.

The skill explains how each of these becomes real.
