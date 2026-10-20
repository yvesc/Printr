# Changelog

All notable changes to Printr are listed here. Versions follow [semantic versioning](https://semver.org/);
each one is a `vX.Y.Z` tag on GitHub, which the server checks to tell the admin a new version is out.

Each minor version is named after a pioneer of printing, in alphabetical order (patch releases keep the
name): Aldus Manutius, Giambattista Bodoni, William Caslon, the Didot family, Robert Estienne,
Pierre-Simon Fournier, Johannes Gutenberg, Richard Hoe, Frederic Ives, Nicolas Jenson…

## [Unreleased]

## [1.9.0] “Jenson” - 2026-10-19

### Added
- Block catalog: **filters** at the top — for kids, offline and free, together, Claude. Search knows them too
  (“kids”, “free”, “together”…).
- The administrator can **hide blocks** from the whole family's catalog (My account → Offered blocks): the
  ones nobody uses. Tickets that already use them keep working.
- When Claude is not available (no API key, no Claude CLI), the blocks that cannot work without it — Claude
  horoscope, word of the day, Claude recipe — leave the catalog on their own. Crosswords and arrow words stay:
  their clues come from the cache.
- Catalog **favorites and recently added blocks follow your account**: you find them on every phone and
  computer. Those kept by the browser until now are moved into the account the first time.
- **Ticket templates**: a new ticket starts from a morning brief, a game break, a kids' ticket or a weekend
  ticket, to change afterwards — or from a first block, as before. Blocks hidden from the catalog are left out.
- **Node-RED**: the [`node-red-contrib-printr`](https://www.npmjs.com/package/node-red-contrib-printr)
  package prints tickets, to-do lists and saved tickets, adds to the shopping list and reads the printer's
  state from Node-RED flows, with only the Pi's address and token to set. See the README.
- **MQTT** (`PRINTR_MQTT_URL`, `mqtts://` for TLS): home automation systems print by publishing a ticket, a
  text, the name of a saved ticket, a to-do list or shopping items to `printr/…` topics, and get the outcome
  on `printr/result`. Printr publishes its state on `printr/status` and its presence on
  `printr/availability` (`offline` as last will). The HTTP rules apply: block limit, hourly quota, queue.
  `mqtts://` checks the broker certificate; `PRINTR_MQTT_CA` trusts a self-signed or private one.
- **Cut** block (`{"type": "cut"}`, alias `coupe`): cuts the paper in the middle of a ticket, so one print can
  come out as several pieces (a coupon to hand over, a list to share). The ticket's `spacing` sets the margin
  above the cut, the next piece starts at the top, and a ticket that ends with it is not cut twice. Shown as
  scissors in the terminal and app previews.

### Changed
- Block catalog: games are split into three groups — grids and numbers, words and letters, challenges and
  creativity.

### Fixed
- Block catalog: blocks no longer show above the search field while scrolling.

## [1.8.0] “Ives” - 2026-10-18

### Added
- **Printer missing? Tickets wait.** The server checks every 10 seconds that the printer is there (USB device
  present, network printer accepting connections). When it is not, tickets — already laid out — go into a
  queue that survives restarts, and print on their own when the printer comes back, schedules included.
  After 12 hours (`PRINTR_QUEUE_HOURS`), a waiting ticket is dropped and the history says why.
- The app shows the printer state (sidebar), a banner with the number of waiting tickets when it is missing,
  a “printer missing, the ticket will come out when it is back” message, and “waiting” / “came out at 9:12,
  late” in the history.
- **Printer stuck? No double ticket.** When the printer stops taking data (cover open, out of paper) for more
  than 30 seconds, the app says so (“cover, paper?”) instead of hanging. The ticket comes out once the printer
  is ready, only once, and the history then shows it as printed. Tickets sent in the meantime wait their turn
  and come out right after it, never mixed with it.
- The history refreshes on its own (every 10 seconds, when you come back to the tab, and when the printer
  state changes): a waiting ticket turns to “printed” without reloading the app.
- **Tonight's sky** block: visible planets with their direction and height, visible International Space Station
  passes until dawn, moon phase and shooting star showers, for your city. Computed locally (JPL orbital
  elements for the planets, SGP4 for the ISS, whose orbit comes from CelesTrak once a day).
- **Recipe** block: paste the address of a recipe, Printr reads its schema.org data (found on most cooking
  sites) and prints the title, servings and times, ingredients to tick and numbered steps, without the ads.
- **Claude recipe** block: name a dish, Claude (Haiku by default) writes the recipe at print time, with the
  same layout; cached per dish and number of servings.
- **Claude without an API key.** When `ANTHROPIC_API_KEY` is not set and the Claude Code CLI is installed,
  Claude blocks go through `claude -p` and its own login (a Claude subscription). `PRINTR_CLAUDE_BACKEND`
  forces `api` or `cli`, `PRINTR_CLAUDE_CLI` gives the program's path; the startup banner and `/api/status`
  (`claude_backend`) say which way is used.

### Changed
- The systemd service may now use up to 768 MB of memory (400 MB before), for the Claude CLI.
- `--preview` on the command line no longer calls Claude, like the app's preview: Claude blocks show a
  placeholder. `--claude` allows the call when you want the real text.
- `scripts/illustrations.sh`: remakes every illustration of the site and README in both languages (block
  examples, app screenshots, showcase tickets, link preview), then builds the site; `--sans-claude` makes no
  Claude call.

### Fixed
- The app recognizes French block names used in JSON tickets (`mots_meles`, `enigme`…) instead of showing them raw.

## [1.7.0] “Hoe” - 2026-10-15

### Added
- Mental math ([#4](https://github.com/XNinety9/Printr/issues/4)): a **beginner** level (sums up to 10,
  subtractions without negative results), a choice of **operations** (+ − × ÷) and of **times tables** to
  practice. Without options, sheets are exactly the same as before.
- **Crossword** block, generated by CrossWords like the arrow words: numbered cells and Across / Down clues in
  English, Roman-numeral rows and “Horizontalement / Verticalement” in French; clues from the CrossWords cache,
  missing ones written by Claude at print time.
- Block catalog: each block's **example ticket**, shown full size with the eye button before adding it,
  **favorites** (★) and **recently added** blocks at the top, and a search that ignores accents and knows
  words in both languages (“meteo” and “weather” both find the weather block).
- History: the administrator can **see each printed ticket** as it came out, for 30 days. Other accounts and
  the scripts token cannot.

- `scripts/appli-demo.sh`: the web app with fake accounts and tickets, no printer and no Claude, to try it
  out in one command.

### Fixed
- Ticket preview: full-width lines (such as the mental math score) are no longer clipped at the end.

## [1.6.1] “Gutenberg” - 2026-10-14

### Security
- rustls updated to 0.23.46, fixing four TLS client advisories (RUSTSEC-2026-0346 to 0349) affecting
  Printr's HTTPS requests (weather, news, Claude API, update check).

### Added
- Version codenames, shown by `printr --version` and in release titles.
- Apache License 2.0 (`LICENSE`), and a NOTICE crediting Printr's author.
- Continuous integration on every pull request: formatting, clippy, tests, web app syntax, shellcheck,
  dependency audit (cargo-deny) and workflow audit (zizmor); Dependabot for weekly dependency updates;
  issue and pull request templates.
- GitHub releases with prebuilt binaries for Raspberry Pi (64-bit, ARMv7, ARMv6) and x86_64 PCs, each
  archive ready to install with `sudo bash install.sh`, plus a `SHA256SUMS` file.

### Fixed
- English website: the example tickets on the home page and in the guide are now the English ones.
- The link preview image and text (Open Graph) are in English.

## [1.6.0] “Gutenberg” - 2026-10-13

### Added
- **English version**: `PRINTR_LANG=en` switches the web app, the command line, server messages and printed
  tickets to English. French remains the default.
- English content for the blocks that need it: riddles and charades, quotes, word search themes,
  workouts, gift coupons, Categories, secret messages, glitch messages.
- Claude writes the horoscope and the word of the day in English.
- Barnum horoscope in English, with Barnum 0.3 or later (`--lang en`). An older Barnum is asked again in
  French, and the whole ticket stays in French.
- Arrow words in English, with CrossWords 1.1.0: English dictionary, `definitions-en.txt` clues and a
  separate grid cache.
- English README and contributing guide (French versions in `README.fr.md` and `CONTRIBUTING.fr.md`);
  bilingual website, with English tickets and screenshots.

### Changed
- CrossWords dependency upgraded to 1.1.0 (JSON format version 2). French grids already cached remain valid.
- The news block also skips English “live”, “watch” and “opinion” items, in both languages.

## [1.5.0] “Fournier” - 2026-10-13

### Added
- Composer: reorder blocks by drag and drop with a handle, by finger or mouse, with auto-scroll near the
  screen edges ([#3](https://github.com/XNinety9/Printr/issues/3)).
- Composer: a “+” between two blocks inserts a new block right there.
- Keyboard: arrow keys on the handle move a block up or down.

### Fixed
- Date fields no longer overflow their block on iPhone.

## [1.4.0] “Estienne” - 2026-10-13

### Added
- Update notice: the server checks GitHub tags (daily by default, `PRINTR_UPDATE_INTERVAL` minutes
  otherwise) and tells the admin when a newer version is out. Nothing is downloaded or installed
  automatically; `PRINTR_UPDATE_CHECK=off` turns the check off.
- Website: a page showing every block with the ticket it prints and its JSON example.

### Fixed
- Website: block tickets shown at their real size, readable on desktop and phone.

## [1.3.0] “Didot” - 2026-10-13

### Added
- **Arrow words** block, generated by the [CrossWords](https://github.com/XNinety9/CrossWords) library:
  numbered grid, clues below, missing clues written by Claude at print time only.
- **Train tracks** puzzle block: link A to B with a single track, from the numbers on the edges.

### Note
- The `v1.2.0` tag was published before the train tracks block was merged and points to an incomplete
  version; 1.3.0 replaces it.

## [1.2.0] “Caslon” - 2026-10-13

Published before the train tracks block was merged: an incomplete version, replaced by 1.3.0.

### Added
- **Arrow words** block (see 1.3.0).

## [1.1.1] “Bodoni” - 2026-10-09

### Fixed
- The page scrolls again over the photo: cropping now happens in a dedicated window.
- The crop frame keeps its exact format on small screens.

## [1.1.0] “Bodoni” - 2026-10-09

### Added
- Crop the photo before printing (portrait, square or landscape; drag, pinch or zoom): only the framed
  area is printed.
- Version number shown in the app, and the logo next to the title on phones.

## [1.0.0] “Aldus” - 2026-10-08

First release.

- Single Rust binary for the Epson TM-T88V (native ESC/POS): command line, web server, scheduler and web app.
- Nearly forty blocks: weather, horoscope, sudoku, riddles, word searches, news from RSS feeds, photos,
  QR codes, shopping list, calendar…
- Family web app: accounts, saved and shared tickets, 24-hour scheduling, little notes with a photo.
- Hardened service, login rate limiting, admin account and per-person print quota.
- One-command installation on a Raspberry Pi, no sudo needed afterwards.
- Windows development with a dev container and a printing relay.

[Unreleased]: https://github.com/XNinety9/Printr/compare/v1.9.0...HEAD
[1.9.0]: https://github.com/XNinety9/Printr/compare/v1.8.0...v1.9.0
[1.8.0]: https://github.com/XNinety9/Printr/compare/v1.7.0...v1.8.0
[1.7.0]: https://github.com/XNinety9/Printr/compare/v1.6.1...v1.7.0
[1.6.1]: https://github.com/XNinety9/Printr/compare/v1.6.0...v1.6.1
[1.6.0]: https://github.com/XNinety9/Printr/compare/v1.5.0...v1.6.0
[1.5.0]: https://github.com/XNinety9/Printr/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/XNinety9/Printr/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/XNinety9/Printr/compare/v1.1.1...v1.3.0
[1.2.0]: https://github.com/XNinety9/Printr/releases/tag/v1.2.0
[1.1.1]: https://github.com/XNinety9/Printr/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/XNinety9/Printr/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/XNinety9/Printr/releases/tag/v1.0.0
