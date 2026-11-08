# Feature naming

ArtCraft apps use descriptive names for their own features. Product names used to describe compatibility, file formats, attribution or external software remain accurate.

## Naming map

Express Tools → Additional Tools. This is a descriptive naming cleanup, not a determination that the old phrase is a registered trademark.

## Compatibility boundary

Command IDs, aliases, prompts, drawing data, DXF/DWG identifiers and interoperability references are unchanged. No assets, attribution, dependencies or rendering behavior change.

## Localization and verification

Each shipped non-English interface catalog includes the new labels. Regression tests cover the labels and stable command identities. Full application tests, platform builds and visual checks are required before these drafts are marked ready.

## Regression coverage

The standard-library-only `display_names` integration tests scan production Rust string literals for the retired display names and preserve explicit compatibility exceptions. They also test identifier, comment and raw-string handling. These focused checks do not replace application or visual testing.
