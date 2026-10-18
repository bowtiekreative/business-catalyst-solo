---
name: catalyst-solo-build
description: Build Catalyst Solo — a desktop control panel for managing one existing website (scan, pages, LAKA sections, content, preview, integrations, tasks/roadmap, Git history, publish) — from the clickable prototype and production brief in this package. Use when asked to implement, scaffold, extend or review Catalyst Solo, or to turn the prototype into a production app.
---

# Catalyst Solo — build skill

You are implementing **Catalyst Solo**: a desktop app that opens one website folder, shows the site as pages, sections and content, and lets a person (or an AI helper) change it safely. Every change is previewed, committed to Git and undoable. Publishing runs the site's existing deploy tool.

## Inputs (read these first, in order)

1. `PRODUCTION_PROMPT_SOLO.md` — the brief. **It wins** whenever it disagrees with the prototype.
2. `prototype/Catalyst Solo.dc.html` — the UX spec. Open it in a browser served from `prototype/` (e.g. `npx serve prototype`); it needs `support.js`, `WebSection.dc.html`, `laka-sections.json` and `brand/` beside it.
3. `prototype/laka-sections.json` — the LAKA section library: 12 families and 128 patterns. Each pattern has `code`, `label`, `variant`, a `formula` and default props `p`. Ship it as the app's section catalog.
4. `prototype/WebSection.dc.html` — the reference renderer for each LAKA `variant`. Port it to real components; don't embed it.

Read the prototype's logic class end to end before writing code. It contains the reference behaviour you must reproduce:
- `applyOps`, `describeOp`, `filesOf`: the change-operation model
- `issues` and `rules`: health checks and LAKA rules
- `checkTask`: task acceptance checks
- `interpret`: the intent → proposal shape that the AI layer must return
- `runPublish`: the preflight order

## Core model — implement exactly

**Site Map** (`.catalyst/site.json`, JSON Schema in `packages/schema`):
```
{ name, domain, stack, pages:[{id,name,route,title,desc,nav,secs:[{id,code,variant,label,formula,p:{…}}]}],
  redirects:[{from,to}], footerLinks:[{label,href}], integrations:[{id,name,via,env:[KEY]}] }
```

**Change operations.** All edits, by humans or AI, are arrays of these ops:
- `set {pid,sid|null,field,from,to}`
- `addSec {pid,at,sec}`
- `delSec {pid,sid}`
- `moveSec {pid,sid,to}`
- `addPage {page}`
- `delPage {pid}`
- `rename {pid,name,route}`
- `redirect {from,to}`
- `delRedirect {from}`
- `delLink {href}`
- `nav {pid,on}`
- `addInt {int}`
- `file {path,isNew}`

One pipeline handles every edit:
1. Validate the ops.
2. Show a review: a human-readable line for each op, from→to diffs, and the files touched.
3. Apply the ops to the Site Map.
4. Write the real source files through the stack adapter.
5. Commit, then auto-push.

Ops must be **pure and reversible**: store `before` and `after` per commit so Undo and Restore work.

**Propagation rules.** These must hold:
- **Rename that changes the route:** update the navigation, rewrite footer and internal links, and add a redirect from old → new.
- **Delete page:** remove it from navigation and add a redirect to `/`.
- **Add page:** update the header navigation and `sitemap.xml`.
- **Add integration:** add its keys to `.env.example`. Real values go to the git-ignored `.env` only.

**Text edits are drafts.** Hold them in a pending map until the user reviews them, then save them as **one commit**. Structural section edits (add, move, remove, duplicate) commit immediately and offer Undo in the toast.

## Build order (one milestone at a time; each ends with green CI on Windows + Ubuntu)

1. **Open and see:**
   - Desktop shell: Tauri (or Electron) + React + TypeScript, plus a local service.
   - Open a folder or clone a repo; run `git init` if needed.
   - Scanner: detect the stack, routes and sections; map blocks to LAKA codes; keep unmatched blocks as *custom* sections.
   - Views: Home, Pages, History, Activity.
2. **Edit safely:**
   - Content view (all text and SEO fields, find & replace, character hints).
   - Review modal and the pending-changes bar.
   - Commit engine with auto-push, Undo and Restore.
   - Preview: run the site's dev server, with draft/live and device widths.
3. **AI helpers:**
   - Provider layer: Claude Code, Anthropic, OpenAI, OpenRouter, and Ollama/LM Studio via OpenAI-compatible endpoints.
   - One helper per view, with a scope limited to its ops (Content = `set` only, Sections = section ops, Pages = page and redirect ops, Integrations = `addInt`).
   - Helpers return `{summary, view, ops, msg}` — the `interpret()` shape. Never write files directly.
   - Tasks: every run is tied to a task; flag drift when the proposal's view ≠ the task's view.
   - Track token use, cost and a monthly budget.
4. **Sections and integrations:**
   - Sections builder: drag from the library, reorder, inspector, live LAKA rules.
   - Forms & integrations: env vars masked, saved to `.env`, synced to the host CLI.
   - Tasks & roadmap: Now/Next/Later, automatic acceptance checks, export `ROADMAP.md`.
5. **Publish and care:**
   - Host adapters: Netlify, Vercel, Cloudflare Pages, GitHub Pages, rsync/SFTP.
   - Preflight in this order: build, LAKA rules, links, secrets, env, unsaved edits, accessibility. Any failure blocks publishing.
   - Publish history with rollback (rollback changes live only, never the draft).
   - Weekly site checks; CLI (`catalyst open|ask|apply|publish|status`); MCP server exposing the same typed tools.

## LAKA rules that block publishing (minimum set)

- `nav.four`: the header must be the first section.
- `attr.footer`: the footer must be the last section.
- `a11y.structure`: exactly one hero (family `02`) per page.
- Broken internal links, missing required env values and unsaved edits also block publishing.
- Missing meta descriptions only produce a warning.
- Extend from `cdn/library/rules.json` in `bowtiekreative/laka`.

## Non-negotiables

- **Nothing changes silently.** Every human or AI change is reviewed before it's applied.
- **Never invent facts:** no made-up testimonials, statistics, prices or client names. Flag LAKA sample copy as placeholder.
- **Secrets:** never committed. Run a secret scan before every commit and block commits that contain secrets. Mask secrets in the UI and logs.
- **The site stays the user's:** plain files, normal Git, and the site keeps working without Catalyst.
- **Plain language** in the UI; show code only on request.
- **Accessibility:** WCAG 2.2 AA throughout — visible labels, 4.5:1 text contrast, full keyboard support.

## Acceptance test (run end to end before calling any milestone done)

Run this on a sample Astro site with a broken `/blog` footer link and two pages missing meta descriptions:
1. Open the folder and scan. Home shows the blockers.
2. Ask the Content helper to "Write the missing meta descriptions". Review, then apply. The commit appears in History, and task t1's criteria flip to **met**.
3. Add a Blog page through the Pages helper. The broken-link blocker disappears.
4. Edit a headline in Content, then review and save as one commit. Undo restores the old text.
5. Publish: preflight passes, the host CLI runs, and Publish history shows the release. Roll back, and live returns to the previous release while the draft keeps the change.

## Deliverables

- Monorepo with `apps/desktop`, `packages/core` (site model, ops, rules), `packages/schema`, `packages/adapters/*` (stacks and hosts), `packages/ai`, `packages/cli`, `packages/mcp`.
- Docs: `docs/ARCHITECTURE.md`, `docs/SECURITY.md` and `docs/DECISIONS.md` (record every assumption there instead of stopping to ask).
- Tests:
  - Unit tests for the ops, rules, scanner and Git engine.
  - A Playwright run of the acceptance test.
  - Fixtures for static HTML, Astro, Next.js and a WordPress theme.

**Start with milestone 1.** Commit `docs/ARCHITECTURE.md` and the Site Map JSON Schema before any feature code.
