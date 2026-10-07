# Contributors in the About window

**About ▸ Contributors** credits everyone who contributed to CADCraft, and **About ▸ Models** credits
the AI models named in `Co-Authored-By` trailers. This follows the shared craftrules standard
[`standards/contributors.md`](https://github.com/storytold/craftrules/blob/main/standards/contributors.md);
this page is the local copy of the decision.

## Decision

- The credits are one generated file, [`contributors/contributors.json`](../contributors/contributors.json).
  `crates/ui-egui/build.rs` turns it into static tables (`$OUT_DIR/credits.rs`) that
  `crates/ui-egui/src/credits.rs` includes. **The credits are compiled into the binary**: nothing is
  read from disk or the network at run time, and the web build has them too.
- Each contributor gets one line: GitHub username (always), display name (opt-in), real name (only
  if they told us), merged PRs, commits, lines added, lines deleted, binary assets added, binary
  assets removed, first and last commit date.
- The list is shown as a **grab bag** (names flowing as text) or a **table**, in the same order. It
  sorts by name, PRs, commits, lines added, lines deleted, line delta, binary assets added/removed,
  first or last commit date. The name toggle cycles **Username → Display name → Real name**; a
  missing name falls back to `@username`. Alphabetical sorting is case-insensitive and ignores the `@`.

## Refreshing

```sh
python3 ../../craftrules/scripts/contributors.py .   # needs git, Python 3.11+, authenticated gh
```

Commit the updated `contributors/contributors.json`. Never hand-edit it: names come only from the
consent registry `craftrules/contributors/people.toml`, model names from
`craftrules/contributors/models.toml`. Git author names and emails are never recorded.
