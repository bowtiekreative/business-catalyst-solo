# Validation status

Passed static checks: Cargo manifests parse as TOML, the Site Map schema parses
as JSON, and every bundled original handoff file is byte-identical to its input.
A credential-prefix scan matched only the detector implementation and synthetic
test inputs. No `.env` file is included. Git whitespace checks passed for all
staged source. Rust formatting still needs rustfmt.

Rust compilation, formatting, linting and tests are pending. Rust was not
installed on the build computer, and Doberman blocked downloading its official
installer because network authentication could not be shown in this session.

Creating `.github/workflows/rust.yml` was also blocked as a sensitive-path write.
No CI workflow has been installed and no green Windows/Ubuntu CI result is claimed.

Run the four commands in the root README after installing Rust. The core tests
cover scan results, framework detection, save/review/commit/undo, stale drafts,
dirty repositories, credential patterns, failed-commit rollback, demo overwrite
protection, symlink boundaries and the original catalog. These are authored tests,
not passed test results. The desktop test draws each empty view without a window.

This starter has not passed the full production brief's Astro acceptance flow.
AI helpers, deployment and live rollback remain unimplemented roadmap items.
