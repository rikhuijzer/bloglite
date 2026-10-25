# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [3.1.0] - 2026-10-24

### Added

- Read the admin password from `BLOGLITE_PASSWORD_FILE` or `--password-file` ([#221](https://github.com/rikhuijzer/bloglite/issues/221)).

### Changed

- Require a non-empty admin password at startup.
- Extend login sessions from two weeks to 30 days.

### Fixed

- Include all posts in the RSS feed via Atom navigation links to older posts ([#77](https://github.com/rikhuijzer/bloglite/issues/77)).
- Extract plain text from Markdown for post descriptions ([#191](https://github.com/rikhuijzer/bloglite/issues/191)).
- Shut down gracefully on `SIGTERM`
- Show the package version with a custom-build or development label in the footer for non-release builds.

## [3.0.0] - 2026-10-17

### Migration from v2

- Change the container image to `zot.huijzer.xyz/bloglite:3` (or pin `:3.0.0`).
- Rename every `FX_` environment variable to `BLOGLITE_`, including backup scripts and CI secrets. The old prefix is no longer read.
- Update health checks and scripts to use `/bloglite` instead of `/fx`.
- Keep the existing `/data` volume mounted to preserve posts, files, and settings.

### Changed

- Rename fx to bloglite, including the binary and Rust packages.
- **Breaking:** replace all `FX_` environment variables with `BLOGLITE_` for both the server and health checks. Update deployment configuration and backup scripts before upgrading.
- **Breaking:** move container images from `rikhuijzer/bloglite` to `zot.huijzer.xyz/bloglite`. Use `/bloglite` for container health checks.

## [2.1.0] - 2026-10-16

### Added

- Jump to file after upload ([#50](https://github.com/rikhuijzer/bloglite/issues/50)).

### Changed

- Run syntax highlighting server-side ([#217](https://github.com/rikhuijzer/bloglite/pull/217)).

## [2.0.1] - 2026-10-05

### Fixed

- Handle malformed search queries without panic ([#215](https://github.com/rikhuijzer/bloglite/pull/215)).
- Handle out-of-range page numbers without panic ([#216](https://github.com/rikhuijzer/bloglite/pull/216)).

## [2.0.0] - 2026-10-01

### Changed

- Show version tag in footer on tagged builds ([#211](https://github.com/rikhuijzer/bloglite/pull/211)).

### Fixed

- Sessions not expiring after two weeks ([#209](https://github.com/rikhuijzer/bloglite/pull/209)).
- Search XSS ([#208](https://github.com/rikhuijzer/bloglite/pull/208)).

### Removed

- Blogroll ([#206](https://github.com/rikhuijzer/bloglite/pull/206)).

## [1.6.3] - 2026-09-15

### Fixed

- Hide deleted posts from the RSS feed ([#203](https://github.com/rikhuijzer/bloglite/pull/203)).
- Preview breaks publish button in Chromium-based browsers ([#204](https://github.com/rikhuijzer/bloglite/pull/204)).

## [1.6.2] - 2026-08-29

### Fixed

- Avoid adding hash when there is none ([#196](https://github.com/rikhuijzer/bloglite/pull/196)).

## [1.6.1] - 2026-07-17

### Fixed

- Escape apostrophe in headings for determining header id ([#192](https://github.com/rikhuijzer/bloglite/pull/192))
- h2-h6 headings causing infinite redirect loop
- Escape filenames in URLs ([#184](https://github.com/rikhuijzer/bloglite/pull/184))

## [1.6.0] - 2026-05-26

### Added

- Show PDFs and other files in the sitemap ([#181](https://github.com/rikhuijzer/bloglite/pull/181)).

## [1.5.2] - 2026-05-12

### Fixed

- Derive post description from post content ([#178](https://github.com/rikhuijzer/bloglite/pull/178)).
- Reduce memory usage.
- Disable various `default-features` in dependencies.

## [1.5.1] - 2026-04-10

### Fixed

- Fix GitHub Actions workflow for tag (release) runs.

## [1.5.0] - 2026-04-05

### Added

- Allow setting log level ([#161](https://github.com/rikhuijzer/bloglite/pull/161)).
- Log main `GET` routes ([#160](https://github.com/rikhuijzer/bloglite/pull/160)).

### Changed

- Updated dependencies

## [1.4.1] - 2026-03-09

### Fixed

- Avoid `href` in header causing invalid HTML in preview ([#156](https://github.com/rikhuijzer/bloglite/pull/156)).
- Show insecure login warning when not using HTTPS ([#149](https://github.com/rikhuijzer/bloglite/pull/149)).

## [1.4.0] - 2026-02-27

### Fixed

- Improved documentation in README.
- Move RSS indexing into separate thread to avoid server becoming unresponsive.

### Added

- Support healthcheck ([#145](https://github.com/rikhuijzer/bloglite/pull/145)).

## [1.3.2] - 2026-02-16

### Fixed

- Avoid unresponsive server during backup ([#109](https://github.com/rikhuijzer/bloglite/issues/109)).

## [1.3.1] - 2026-02-04

### Fixed

- Avoid empty slug causing 404 error ([#136](https://github.com/rikhuijzer/bloglite/pull/136)).
- Mitigate API timing attacks ([#135](https://github.com/rikhuijzer/bloglite/pull/135)).
- Mitigate SQL injection for `Kv` database.
- Improve cookie security ([#133](https://github.com/rikhuijzer/bloglite/pull/133)).
- Disallow framing the page in an iframe.
- Avoid infinite redirect on empty slug ([#137](https://github.com/rikhuijzer/bloglite/pull/137)).

### Changed

- Updated dependencies
- Moved from Debian 12 to 13 ([#134](https://github.com/rikhuijzer/bloglite/pull/134)).

## [1.3.0] - 2025-11-14

### Added

- Allow setting site description via settings ([#123](https://github.com/rikhuijzer/bloglite/pull/123)).
- Support triggering Forgejo instances for backups ([#116](https://github.com/rikhuijzer/bloglite/pull/116)).

### Changed

- Set extra HTML head via settings instead of environment variable ([#122](https://github.com/rikhuijzer/bloglite/pull/122)).
- Removed RSS button and button text when not having blogroll ([#118](https://github.com/rikhuijzer/bloglite/pull/118)).

## [1.2.1] - 2025-10-24

### Fixed

- Avoid redirect URL crashing on trailing newlines.

## [1.2.0] - 2025-09-13

### Fixed

- Avoid server getting stuck on requesting a backup ([#98](https://github.com/rikhuijzer/bloglite/pull/97)).
- Avoid post deletion causes the search to break ([#100](https://github.com/rikhuijzer/bloglite/pull/100)).
- Let the URL with slug be the canonical URL ([#103](https://github.com/rikhuijzer/bloglite/pull/103)).

### Changed

- Allow admin to disable dark mode ([#101](https://github.com/rikhuijzer/bloglite/pull/101)).

## [1.1.4] - 2025-08-05

### Fixed

- Do not disable form submit button on empty settings fields ([#89](https://github.com/rikhuijzer/bloglite/pull/89)).

## [1.1.3] - 2025-07-28

### Changed

- Updated dependencies ([#84](https://github.com/rikhuijzer/bloglite/pull/84)).

## [1.1.2] - 2025-06-18

### Fixed

- Disable form submit button if textarea is empty ([#78](https://github.com/rikhuijzer/bloglite/issues/78)).

### Changed

- Updated dependencies, particularly rusqlite from 0.35 to 0.36.

## [1.1.1] - 2025-06-02

### Fixed

- Fix horizontal rule (`---`) in content.

## [1.1.0] - 2025-05-28

### Added

- Pagination for posts on the homepage.

### Fixed

- Allow seeing full feed items on blogroll even when long.

## [1.0.4] - 2025-05-24

### Fixed

- Fix math display in preview when only inline math is present.

## [1.0.3] - 2025-05-24

### Fixed

- Fix code block highlighting in preview.
- Show inline math in preview.
- Remove short URL from below posts.
- Fix login/logout link on some pages.

## [1.0.2] - 2025-05-22

### Added

- Add copy slug button below posts ([#68](https://github.com/rikhuijzer/bloglite/pull/68))
- Allow slug behind URL (for example, `/posts/1/my-post`)

### Fixed

- Open Graph description for homepage.
- Open Graph title.
- Open Graph type.
- Filter old items from blogroll.
- Improve `domain_from_url`.

## [1.0.1] - 2025-05-17

### Fixed

- Init blogroll setting to avoid crash on startup

## [1.0.0] - 2025-05-17

### Added

- Blogroll ([#63](https://github.com/rikhuijzer/bloglite/pull/63))

### Fixed

- Fix `\n` instead of newline in rss ([#65](https://github.com/rikhuijzer/bloglite/pull/65))

[3.1.0]: https://github.com/rikhuijzer/bloglite/compare/v3.0.0...v3.1.0
[3.0.0]: https://github.com/rikhuijzer/bloglite/compare/v2.1.0...v3.0.0
[2.1.0]: https://github.com/rikhuijzer/bloglite/compare/v2.0.1...v2.1.0
[2.0.1]: https://github.com/rikhuijzer/bloglite/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/rikhuijzer/bloglite/compare/v1.6.3...v2.0.0
[1.6.3]: https://github.com/rikhuijzer/bloglite/compare/v1.6.2...v1.6.3
[1.6.2]: https://github.com/rikhuijzer/bloglite/compare/v1.6.1...v1.6.2
[1.6.1]: https://github.com/rikhuijzer/bloglite/compare/v1.6.0...v1.6.1
[1.6.0]: https://github.com/rikhuijzer/bloglite/compare/v1.5.2...v1.6.0
[1.5.2]: https://github.com/rikhuijzer/bloglite/compare/v1.5.1...v1.5.2
[1.5.1]: https://github.com/rikhuijzer/bloglite/compare/v1.5.0...v1.5.1
[1.5.0]: https://github.com/rikhuijzer/bloglite/compare/v1.4.1...v1.5.0
[1.4.1]: https://github.com/rikhuijzer/bloglite/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/rikhuijzer/bloglite/compare/v1.3.2...v1.4.0
[1.3.2]: https://github.com/rikhuijzer/bloglite/compare/v1.3.1...v1.3.2
[1.3.1]: https://github.com/rikhuijzer/bloglite/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/rikhuijzer/bloglite/compare/v1.2.1...v1.3.0
[1.2.1]: https://github.com/rikhuijzer/bloglite/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/rikhuijzer/bloglite/compare/v1.1.4...v1.2.0
[1.1.4]: https://github.com/rikhuijzer/bloglite/compare/v1.1.3...v1.1.4
[1.1.3]: https://github.com/rikhuijzer/bloglite/compare/v1.1.2...v1.1.3
[1.1.2]: https://github.com/rikhuijzer/bloglite/compare/v1.1.1...v1.1.2
[1.1.1]: https://github.com/rikhuijzer/bloglite/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/rikhuijzer/bloglite/compare/v1.0.4...v1.1.0
[1.0.4]: https://github.com/rikhuijzer/bloglite/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/rikhuijzer/bloglite/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/rikhuijzer/bloglite/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/rikhuijzer/bloglite/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/rikhuijzer/bloglite/releases/tag/v1.0.0
