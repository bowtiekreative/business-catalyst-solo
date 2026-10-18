# Build prompt: Catalyst Solo — manage the website you already built

You are the lead engineer building a production-ready app. Read this whole brief before writing code. Write any ambiguity into `docs/DECISIONS.md` with the assumption you made, and keep going.

---

## 1. What we're building

A **single-project control panel** for one person who has just built a website (by hand, with an AI agent, or with a site builder that exports code) and now needs to **understand it, change it safely and keep it running**.

The user opens one folder (their site) and gets a visual dashboard of it: pages, sections, content, data, settings, history and deployment. AI helpers make changes on request. Every change is shown, explained and saved to Git, and can be undone.

This is the solo edition of Project Business Catalyst. Leave out workspaces, teams, multi-project management, client roles and agency features. If you're unsure whether to include something, ask: *does one person managing one live website need this?*

### Who it's for
- Someone whose agent built them a site and who can't see what the agent did.
- A freelancer or small-business owner who wants to edit their own site without breaking it.
- A developer who wants a calm, visual overview of a small project.

---

## 2. Source material

1. **Prototype:** `Build Studio.dc.html`. It's the UX reference. Reuse its look, tone and these views: Pages, Sections, Templates, Design, Records (only if the site has data), Sign-in (only if the site has accounts), Preview, Code, Versions, Deploy, Activity. Hide or drop: Add-ons marketplace, Access matrix with many roles, API keys for teams, Tests per role.
2. **LAKA design system:** `github.com/bowtiekreative/laka`, `laka-sections.json` and `WebSection.dc.html`. Use them for the sections library and rule checks.
3. **TheBigBang:** `github.com/bowtiekreative/TheBigBang`. Borrow only snapshots (moons), environments (planets: just Live and Draft), audit log and validation.

---

## 3. Principles

1. **One site, one folder.** No workspace concept. The app opens a folder, like a text editor.
2. **The site stays yours.** Plain files in a normal Git repo. Uninstalling the app leaves the site untouched and working.
3. **See before you save.** Every change, whether human or AI, shows a preview and a plain-language summary before it's applied.
4. **Everything is undoable.** Each applied change is a Git commit; "Undo" reverts it; snapshots mark known-good states.
5. **Works with what you have.** Any stack, any host, any AI provider. It manages your existing deploy method; it never hosts.
6. **Plain language.** Explain code concepts in everyday words. Show the code on request, never force it.

---

## 4. Install and open

- Desktop app for Windows (first-class) and Linux (preferred); macOS best effort. One installer, no Docker required.
- First run: **"Open your website folder"** or **"Clone from GitHub/GitLab"**. If the folder isn't a Git repo, offer to create one and make an initial commit.
- Optional headless mode for a Linux VPS: run as a service bound to localhost, reached over an SSH tunnel.
- Per-site settings live in `.catalyst/` inside the site folder (committed, except secrets). App settings and keys live in the OS keychain.
- Suggested stack for the app itself: Tauri (or Electron) + React + TypeScript, plus a small local service for the file watcher, Git and agents. Record the choice in `DECISIONS.md`.

---

## 5. Understanding the site (import scan)

The first job is **making an existing site visible**. On open, a scanner agent reads the folder and builds a **Site Map** (`.catalyst/site.json`):

- **Stack detection:** static HTML, Astro, Next.js, Nuxt, SvelteKit, Hugo, Jekyll, Eleventy, WordPress theme, PHP, Express, or "unknown". Record build and dev commands and the output folder.
- **Pages and routes:** each with title, URL, template file, and whether it's live.
- **Sections:** each page split into recognizable blocks (header, hero, features, pricing, FAQ, footer…), matched to the closest LAKA pattern where possible. Blocks that don't match are kept as **custom sections** — editable as code, never rewritten without asking.
- **Content:** editable text, images, links and SEO metadata per page, mapped to where they live in the code.
- **Data (if any):** database type, tables and fields, read from migrations, ORM models or a live connection the user provides. If there's no database, say so and hide data views.
- **Forms, integrations and environment variables** in use, without reading secret values into logs.
- **Health baseline:** broken links, missing alt text, contrast failures, missing meta tags, page weight, outdated dependencies, exposed secrets.

Show the result on a **Home** screen: what the site is, what it's built with, what's healthy and what needs attention. Anything the scanner couldn't understand is listed plainly with its file path, not hidden.

---

## 6. Views

Each view reads from and writes to the real files, through the Site Map.

1. **Home:** summary, health score, latest changes, task list, "what's live vs. draft".
2. **Pages:** list and navigation order; add, rename, hide and redirect pages; system pages (404, maintenance).
3. **Sections:** drag-and-drop page builder with the LAKA library, an inspector for copy and images, and live LAKA rule checks (blocking issues stop publishing).
4. **Content:** a spreadsheet-like editor for all text, images and SEO fields across pages. Includes find & replace with preview.
5. **Design:** colours, type, spacing and buttons as tokens; logo and favicon; custom CSS. Shows contrast checks live.
6. **Data:** only if a database or CMS exists. Browse, search, edit and export records with validation. Schema changes go through a migration preview.
7. **Forms & integrations:** where each form sends, plus analytics, email and payment keys. Adding an integration also creates its env vars (§9).
8. **Preview:** the real site running locally (via the detected dev command), at desktop, tablet and phone sizes, with Draft vs. Live toggle.
9. **Code:** file tree, diff of every change, and "open in my editor" (VS Code, Cursor, etc.).
10. **History:** Git timeline in plain language, snapshots, compare, restore.
11. **Publish:** run the site's existing deploy method (§10), with a preflight checklist and rollback.
12. **Activity:** every human and AI action with time, summary and linked commit.
13. **Tasks & roadmap:** see §8.

---

## 7. AI helpers

- **Bring any model:** Claude Code, Anthropic, OpenAI, OpenRouter, or a local model (Ollama, LM Studio, any OpenAI-compatible endpoint). Choose one default, optionally a different one per view. Show token use and cost; allow a monthly budget.
- **A helper in each view** ("Ask AI" panel). It can only change what that view owns, for example the Content helper edits copy, not layout. Cross-view changes go through a request the user approves.
- **Whole-site requests** from Home ("add a pricing page", "make the site faster") are broken into tasks first (§8), then run view by view.
- **Every AI change is a proposal:** visual preview, plain summary, file diff. The user can apply, edit or discard it. No silent changes.
- Helpers use typed tools only (`readSiteMap`, `proposeChange`, `runChecks`, `preview`, `updateTask`); no free-form shell access by default.
- **Never invent facts.** No made-up testimonials, statistics, prices or client names (LAKA `content.claim`, `data.source`).
- Changes propagate: renaming a page updates navigation, internal links and the sitemap, and suggests a redirect. A new data field shows up in Content and Forms.

---

## 8. Tasks & roadmap

Keeps the user and the AI focused on what was asked.

- The user (or the AI, from a request) adds tasks: title, what "done" means, status (`todo`, `doing`, `review`, `done`), linked commits.
- Each AI run is tied to one task and must say which "done" criteria it met; the app checks what it can (rule checks, links, build).
- Flag AI changes unrelated to the current task.
- **Roadmap view:** tasks grouped into "Now / Next / Later", exported as `ROADMAP.md` in the repo.

---

## 9. Git, saving and environment

- **Every applied change makes a commit**, with a plain message like `content: update homepage headline`; the author is the user or `ai:<view>@<model>`.
- **Auto-push** to the remote (if one is set) after each commit; it can be turned off. Failed pushes are shown and retried; no work is ever lost.
- **Draft vs. Live:** a simple two-branch model (`draft` and `main`) hidden behind plain wording. "Publish" merges draft into main and runs the deploy.
- **Snapshots** are Git tags with a name and note.
- **Environment variables:** generate `.env.example`, keep real values in a git-ignored `.env` or the host's secret store. Add variables automatically when an integration needs them, generate strong secrets, mask them in the UI. Export in the format the host expects (`.env`, host CLI, GitHub Actions secrets, PowerShell or shell script).
- Pre-commit secret scan; block commits that contain secrets.

---

## 10. Publishing (manage, don't host)

Detect or let the user pick how the site goes live, then drive that tool:

- GitHub Pages, Netlify, Vercel, Cloudflare Pages, Render, Fly (through their CLIs or Git push)
- FTP/SFTP or rsync over SSH to shared hosting or a VPS
- Docker or systemd + nginx/Caddy on a VPS (generate the config if missing)

Before every publish, run the **preflight checklist**: build succeeds, no blocking LAKA rules, no broken internal links, no secrets in the build, required env vars present, Lighthouse-style checks for speed and accessibility. Keep a publish history with one-click rollback.

---

## 11. Security & site care (out of the box)

- Security headers, HTTPS check, dependency audit, exposed-file check (`.env`, `.git` on the server), form spam protection, and if the site has accounts, secure sign-in defaults (as in the prototype's Sign-in tab).
- A weekly **site check** (when the app is open, or via the VPS service): uptime ping, SSL expiry, broken links, outdated packages. Shown on Home as plain-language to-dos, each of which can become a task.
- Automatic backups: the Git repo plus, if there's a database, a scheduled export to a folder the user chooses.

---

## 12. Remote control

A small local API and CLI so the site can be managed without the dashboard:
- `catalyst open .`
- `catalyst ask "change the hero headline to …"`
- `catalyst apply <id>`
- `catalyst publish`
- `catalyst status`

Also an **MCP server** exposing the same tools, so an outside agent stays inside the same rules. Bind to localhost; use a token for anything remote.

---

## 13. Milestones

1. **Open & see:** desktop shell, open/clone a folder, Git init, import scan, Home, Pages, Code, History, Activity.
2. **Edit safely:** Content and Design editors, Preview (local dev server), proposals with diffs, Undo, auto-commit/push.
3. **AI helpers:** provider layer (Anthropic, OpenAI, OpenRouter, Ollama, Claude Code), per-view helpers, Tasks & roadmap.
4. **Sections & data:** LAKA section builder with rule checks; Data view for detected databases or CMSs; Forms & integrations with env vars.
5. **Publish & care:** deploy adapters, preflight, rollback, site checks, backups, CLI, MCP.

Each milestone ends with passing CI on Windows and Ubuntu, a demo script in `docs/demos/`, and tests that open real sample sites (static HTML, Astro, Next.js, WordPress theme).

---

## 14. Quality bar

- TypeScript strict; ESLint and Prettier; unit tests for the scanner, Site Map, Git engine and env generator; Playwright end-to-end tests for the main flow (open site → edit headline → preview → publish → undo).
- Generated or edited code must pass the site's own linter and formatter, and must keep the site's existing style.
- WCAG 2.2 AA throughout the app.
- Docs: `README.md`, `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/DECISIONS.md`.

---

## 15. Not in scope

Teams and roles for multiple editors, multiple projects per window, client billing, hosting, and building brand-new apps from a blank prompt. A future "Start a new site" flow can reuse the full Catalyst pipeline.

**Start with Milestone 1.** Commit `docs/ARCHITECTURE.md` and the JSON Schema for `.catalyst/site.json` before writing feature code.
