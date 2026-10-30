# Release notes guide

Rules for rewriting the newest `CHANGELOG.md` section on a Release PR. Edit `CHANGELOG.md` only.

## Audience

People who install wglane, a macOS command-line tool that runs NordLynx/WireGuard tunnels alongside Tailscale. They run `brew install` and then `wglane up`. They care about what they can now do, what stopped breaking, and anything they must change. They do not care about internal refactors.

## What to keep

- Keep the version heading and its compare link exactly as release-please wrote them.
- Use only facts from the commit subjects and merged PR titles you were given. Never invent PR numbers, flags or behaviour.
- If an entry came from a hand-written `[Unreleased]` section, fold it into this section and delete the `## [Unreleased]` heading and its items, so nothing appears twice.

## Layout

1. One to three sentences on the theme of the release.
2. `### What's New`: one `####` per user-visible feature. Say what was awkward before, what changed, and link the PR if known.
3. `### Fixes`: one bullet per fix, written as the symptom that is gone.
4. `### Smaller changes`: everything else worth one line. Omit internal-only work.
5. Omit any section that would be empty.

## Voice

- Plain words, short sentences, present tense.
- No "seamless", "powerful", "robust", "leverage", and no closing flourish.
- At most two em dashes in the whole section.
- Say "command" or `wglane up`, not "functionality".

## Check before finishing

- The heading and compare link are unchanged.
- No text was added that the subjects do not support.
- Nothing outside the newest section was touched.
- No credentials, host names, local paths or private references appear.
