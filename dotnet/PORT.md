# Porting Armory to C# and WinUI 3

The whole application, core and shell, re-implemented in C#. The Rust workspace
stays in the repo as the reference implementation and as the server: nothing
here is a shell over `armory-core`, and there is no FFI. `armory-server` stays
Rust on the NAS, and the C# client speaks its wire format unchanged, so one
account keeps moving between the Linux machine, the NAS and this one.

The addon (`addon/Armory_Collector/`) and the corpus (`data/`) are shared, not
ported. The Lua is read by both implementations; the JSON is embedded in both
binaries.

## Decisions

- **.NET 10, WinUI 3 on Windows App SDK 1.8, built from the dotnet CLI.** No
  Visual Studio. `dotnet build` restores the XAML compiler and the Windows SDK
  through NuGet; a probe project proved it on 2026-09-14.
- **Entity Framework Core over SQLite** is the store, and it is EF all the
  way down: the schema is the model and an EF migration, reads are LINQ,
  writes go through the change tracker and `SaveChanges`, bulk statements are
  `ExecuteDelete` and `ExecuteUpdate`, and the sync's generic row apply walks
  the model's metadata (entity by table name, property by column name) rather
  than carrying twenty-seven copies of itself. Exactly two things are raw SQL
  because EF has no model for them: the change-log triggers, generated from
  `Sharing.Tables` and applied at open, and the PRAGMA. Same table and column
  names as `core/src/store.rs`, because the wire's rows are *positional* over
  `sync::TABLES` and the Rust server holds the same tables. The change tracker
  is what makes "a repeat write enqueues nothing" free: an entity whose values
  did not change produces no UPDATE, so the triggers never see it.
- **One writer, one unit of work at a time.** The recording flag lives in the
  database file, so exactly one thread drives a `Store`. Every public
  operation runs against the context, saves, and clears the change tracker,
  so a context that lives as long as the application does not accumulate the
  account. The UI and the network never touch the context.
- **Typed results, not exceptions.** `Result<T, E>` in `Armory.Core.Result`,
  with exceptions caught only at the seams: SQLite, `HttpClient`, the file
  system. Every `Outcome` variant the Rust has (`Unchanged`, `Empty`, `Stale`)
  keeps its meaning and its distinctness.
- **Vertical slices.** `Armory.Core` is organised by capability, not by layer.
  A slice holds its model, its store queries as extension methods over the
  context, its request builders and its parsers. The store's schema and the
  sharing machinery are the two things that are genuinely cross-cutting and
  live under `Store/` and `Sharing/`.
- **Tests are ported test for test** from the Rust, keeping the Rust name as the
  display name, so the two suites can be diffed. The 393 core tests are the
  contract; a C# module is done when its Rust module's tests pass against it.
  Real SQLite in a temp file, real Lua files in a temp directory, a real clock
  with an injected instant. `IRemote` is the one interface, because the other
  end is a socket.
- **The GTK design is not carried over.** The shell is the Claude Design
  handoff in `~/Downloads/Armory app UI redesign.zip` (see the README inside):
  Fluent tokens, `NavigationView`, page header with the primary action,
  `InfoBar` for standing conditions, asides as cards. Copy comes from
  `src/ui/*.rs`. Two corrections to the handoff: the redirect URI is
  `http://localhost:21451/callback`, and there is no global search.
- **Dependencies, and why.** `Microsoft.EntityFrameworkCore.Sqlite` (asked
  for). `Microsoft.WindowsAppSDK` (the platform). `CommunityToolkit.Mvvm`
  (source-generated `INotifyPropertyChanged` and commands; without it every
  page carries two hundred lines of boilerplate that is not about Armory).
  `xunit.v3` for tests. Nothing else without a line here saying why.

## Layout

```
dotnet/
  Armory.sln
  Directory.Build.props        warnings are errors, nullable, analysers
  Directory.Packages.props     one version per package
  test.ps1                     format check, build, test — the gate
  Armory.Core/                 no Windows types; runs on any .NET
    Result.cs                  Result<T,E>, Outcome
    Store/                     DbContext, entities, migrations, Apply, Purge, Reconcile
    Sharing/                   Tables registry, Row/Parcel/Applied/Pulled, Replica, IRemote
    Addon/                     Lua reader, collector and chronicle files
    Roster/                    character, cohort, enrolment
    Run/                       run, plan, achievement, hunt, criteria
    Provenance/                earned reputation and currency
    Tally/
    Chronicle/                 session, digest, journal brief and parse
    Collections/               collectibles, rarity
    Market/                    auctions, prices, worth_making, worth_selling
    Zones/                     place, adventure, the embedded corpus
    Blizzard/                  oauth, profile, collections, gamedata, media, auctions — builders and parsers only
    Settings/
  Armory.Core.Tests/           xunit v3; one file per Rust module, same test names
  Armory.App/                  WinUI 3 shell: pages, view models, Http, Sync transport, Images, Vault, Redirect
  Armory.SyncCheck/            console: two stores and a real server, the sync-check.sh equivalent
```

## Phases

Each phase is: read the Rust module, port its tests first, port the module
until they pass, run `test.ps1`. A phase is not done while a Rust test has no
C# counterpart or a counterpart is skipped.

- [x] **0. Toolchain and skeleton.** .NET SDK, Windows App Runtime, probe
      build, solution, `test.ps1`, Serena's C# language server.
- [x] **1. Sharing contract.** `Sharing/Tables.cs` from `sync::TABLES`, the
      wire types, `Weight`, base64. All twelve `sync.rs` tests, plus one that
      reads a pull in the shape the Rust server writes.
- [x] **2. Store foundation.** Entities and context, the initial migration,
      the triggers, the recording flag, machine id and cursor, `Apply` with
      every `Rule` and `Guard`, `Purge`, `Reconcile`, the response cache and
      the watch list. Twenty-two of `replica.rs`'s twenty-six tests and the
      response-cache and watch-list tests from `store.rs`; the four that need
      `save_collected`, `save_run`, `record_snapshot` and `save_owned` land
      with those slices, and `a_second_identical_write_enqueues_nothing` is
      the one that matters most among them.
- [x] **3. Replica and transport.** `NextStep`, `AbsorbPush`, `AbsorbPull` and
      `Pass`, with an in-process three-store convergence test.
      `Armory.Client/Sharing/HttpRemote` speaks `/push`, `/pull`, `/wait`,
      `/accounts`, `/health` with the three headers, ported test for test
      from `ui/sync.rs` plus fake-server tests for each route.
      `Armory.SyncCheck` is the port of `examples/sync-check.rs`, working in
      its own `sync-check` account and deleting it afterwards. **Run against
      the NAS on 2026-09-14: sixteen checks, all passed**, against the Rust
      `armory-server` image `2026-08-17-0e71c73`. Run it again after anything
      that touches sharing: `SYNC_CHECK_URL=http://nas.example.ts.net:8084`,
      `SYNC_CHECK_TOKEN` from the server's environment, `SYNC_CHECK_A` and
      `SYNC_CHECK_B` two scratch directories, then
      `dotnet run --project Armory.SyncCheck`. The checks marked
      `TODO(slices)` fill in as sessions, the roster, tallies and runs are
      ported.
- [~] **4. Addon.** Done: the Lua reader, the file layout, the collector
      reader (account and character files, with this week's flavour, vault
      and decor), `Settings`, `Tally`, `Provenance`, `Roster` and `Cohort`,
      the `Detail` record with the Rust's exact JSON, and the store's writers
      and readers for all of it (`RosterStore`, `CollectedStore`,
      `CatalogueStore` with the toy collapse). The replica tests that waited
      on these are in: `a_counter_never_goes_backwards_whichever_side_is_behind`
      and `a_second_identical_write_enqueues_nothing` (the run and the
      snapshot join the latter when their slices land). Still to do: the
      addon's chronicle reader, which goes with phase 6, and a smoke test
      against the real SavedVariables on this machine.
- [x] **5. The run.** `Run/Criterion.cs`, `Evaluation.cs`, `Run.cs` (with
      the serde shapes of `Standing`, `Bucket` and `Baseline` as converters,
      because `goal.standing`, `goal.bucket` and `run.baseline` are columns
      that travel), `Plan.cs`, `Collections/Hunt.cs`, and `Store/RunStore.cs`.
      Every test from `achievement.rs`, `run.rs`, `plan.rs` and `hunt.rs`, and
      the two replica tests that waited on `save_run`. One of them caught a
      real difference: the replica's row lookup went through the primary key,
      and `run` travels under `key`, not `id`. It now queries the wire key
      columns for every table.
- [x] **6. Chronicle, zones, collections.** `Chronicle/Session.cs`,
      `Digest.cs`, `Journal.cs` (the brief, the request, `strip_thinking`),
      `Prose.cs`; the addon's chronicle reader with this week's three
      happenings; `Zones/Place.cs` and `Adventure.cs` over the embedded
      corpus; `Collections/Rarity.cs`; `Store/ChronicleStore.cs` and
      `ZoneStore.cs`. Every test from `chronicle.rs`, `source/journal.rs`,
      `addon/chronicle.rs`, `place.rs`, `adventure.rs` and `rarity.rs`.
- [x] **7. Blizzard and market.** `Blizzard/Api.cs`, `OAuth.cs`, `Profile.cs`,
      `Collections.cs`, `Media.cs`, `GameData.cs`, `Auctions.cs`, `Source.cs`
      (`Request`, `Reason`, `Outcome`), `Market/Market.cs` and `Recipe.cs`,
      `Store/MarketStore.cs` and `ItemStore.cs`; the client's `Http` with the
      rate gate and Blizzard's reading of 401/403/404, and `Images` with the
      LRU and the thirty-day sweep. Every test from `source/blizzard/*`,
      `market.rs`, `ui/http.rs` and `ui/images.rs`.
- [x] **8. Shell.** `Armory.Client/Shell/` is the port of the GTK application
      object with no window in it, as partial classes of one `Account`:
      `Account.cs` (restore, the addon watch with its settle delay, the
      collected read, enrolment, the run's start/replan/remeasure/attest/
      exclude, the sharing pass with its debounce, backstop and park, the
      dialog's state), `Account.Http.cs` (the two clients, the token, the
      generation, the response-cached fetch), `Account.Blizzard.cs` (sign-in
      through the loopback listener in `Blizzard/Redirect.cs`, the token in
      the vault, the sync fan-out: cohort, inputs then catalogue, collections,
      media, item names, guide; the art maps and reputations restored from
      the response cache), `Account.Chronicle.cs` (sessions kept, the journal
      identified and written through a local llama-server **or through the
      Claude Code command-line** — see below — automatic writing, the queue
      abandoned on the first failure), `Account.Market.cs` (the
      market sync, the price net, realm and item watches, `worth_making`,
      `worth_selling`, the Warband summary) and `Account.Art.cs` (the image
      cache with its own client, the Rarity odds read once per install). The
      single-threaded `StoreWorker`, the `ISecrets` seam and `Paths` beside
      them. `Armory.App` holds the `NavigationView` window in the GTK order
      with live tallies, the handoff's tokens as styles, the Password Vault
      secrets, every page (Run, Chronicle, Zones, the four collections,
      Roster, Character, Reputations, Market, Onboarding, Settings), the
      Account & Sharing, watch, journal, achievement and collectible dialogs,
      all built in code from `Widgets`, and `Shell/Preview.cs`. The shell's
      own tests run the orchestrator against real stores, an in-process
      server and a fake `HttpMessageHandler`. Two departures from the GTK,
      both deliberate: a sync merges its answers once everything has landed
      rather than redrawing per answer (the single-threaded contract, and
      deterministic tests), and the redirect listener is a raw `TcpListener`
      because `HttpListener` needs a URL reservation a non-administrator does
      not have.
- [x] **9. Install.** `publish.ps1` makes a self-contained unpackaged build
      under `dotnet/publish/`; `install.ps1` puts it under
      `%LOCALAPPDATA%\Programs\Armory` with a Start Menu shortcut and
      `uninstall.ps1` reverses that, the pair being the Windows half of
      `install.sh`; `SETUP.md` has the Windows section.
- [x] **10. The journal through Claude Code.** The one thing here the Rust
      does not have. `Settings.JournalBackend` chooses between the
      llama-server and the `claude` command-line signed in on this machine,
      `Settings.JournalModel` is the alias it is asked for (`sonnet` unless
      said otherwise), and both are in the Rust `Settings` too — with
      `deny_unknown_fields` a file written here would otherwise be thrown
      away whole over there. `Chronicle/Journal.cs` grew `Command`, the
      subprocess counterpart of `Request`: `Compose` builds
      `claude -p --output-format json --json-schema … --tools "" --model …
      --system-prompt <the voice>` with the same brief on stdin that the
      server gets in its message, `ParseComposed` reads the entry out of
      `structured_output` (or the text result when that is all there is) and
      names the model that produced most of the reply, and `AuthStatus` /
      `ParseAuthStatus` are the readiness check — `claude auth status` says
      who is signed in and never shows a token. `Shell/ClaudeCode.cs` runs
      it: PATH first and then `~/.local/bin`, an empty working directory of
      Armory's own under `%LOCALAPPDATA%\Armory\journal`, `ANTHROPIC_API_KEY`
      and `ANTHROPIC_AUTH_TOKEN` withheld from the child, UTF-8 pipes, the
      journal's timeout, the process tree killed on it. `Account` takes the
      runner the way it takes the HTTP handler, so the shell tests drive a
      fake CLI. Settings → Journal and the journal dialog offer the choice.
      Run for real on 2026-09-14 against the Max login on this machine: the
      sign-in check from inside the app, and one entry composed through the
      real seam in sixteen seconds, named `claude-sonnet-5`.
- [x] **11. Tooling.** `install-addon.ps1` is the port of `install-addon.sh`:
      finds the install where `Settings.WowSearchPaths` looks, reads every
      client's version out of `.build.info`, and stamps the `.toc`'s
      `## Interface:` line with the whole list before copying the addon into
      every `_*_` client. `Armory.Inspect` is the port of
      `examples/inspect.rs`, the counts that tell "the parser produced
      nothing" from "the account has nothing"; it takes the account folder as
      an optional second argument because the launcher can leave a numeric
      folder that sorts ahead of the real login. Both run on 2026-09-14
      against this machine's install.
- [x] **12. Parity with the GTK shell.** A file-by-file comparison on
      2026-09-14 found the core complete and the shell thinner in places;
      each gap was then closed. The Run page's four-tab goal browser (To do,
      Your word, Done, Spent, membership ported clause for clause from
      `Tab::holds`), goal search and the day line. The Character page's
      keystones, Great Vault, raids and lockouts, dated history, people,
      logout footnote, the six record tiles with the gold split, and the
      portrait in a class ring (`Widgets.Portrait`, shared with the Roster).
      The Collection pages' "closest to earning" with the region's reset
      day and the odds and tries, and "where they come from"
      (`Client/Collections/`). The Roster's portraits, ILVL and Mythic+
      columns, headline and subtitle (`Client/Roster/`). Window chrome: Sync
      now with its ring and About Armory in the navigation footer, Ctrl+R and
      Ctrl+Q. The GTK's rail toggle is deliberately absent — the design has
      no rail. The UI tests that had no counterpart came across with their
      logic pulled into `Armory.Client` so they need no window: the sync
      dialog's wording (11), the roster's (5), the collection page's (5) and
      the art-budget test, the collectible links (4), the almanac's prose
      (3), and four chronicle integration tests over a real file and a real
      store. Not ported, on purpose: the almanac's colour/CSS and Pango
      paragraph tests, and the three GTK gesture tests. `Sample.cs` is the
      port of the preview's made-up account, and `ARMORY_PREVIEW_SAMPLE=1`
      paints from it with a width check on every page. The one bug the
      sample preview found was in the new Run page — the browser panel
      re-parented on every redraw — and it is fixed. 575 tests.

## Validated on 2026-09-14

- The gate: 528 tests, every Rust test with a counterpart, green.
- `Armory.SyncCheck` against the NAS's `armory-server`: twenty-three checks,
  all passed, in the throwaway `sync-check` account.
- The app itself, pointed at the NAS's `default` account: pulled the whole
  account (157,719 rows, seven minutes on the first pass, 40 MB of store),
  then read this machine's own collector files and shared them back. Every
  page painted with the real account in both themes through `ARMORY_PREVIEW`,
  and the live window captured through `PrintWindow`; mount and pet renders
  arrived through the image cache. Not seen populated, for want of data on
  this machine rather than code: a Battle.net sign-in (no client registered
  here), reputations from the API, toy and decor icons and the market's item
  names (both need the API). The journal was the other one, and phase 10
  closed it: no llama-server runs here, but Claude Code is signed in, and an
  entry has been composed through it. The gate is 539 tests after it.

## Validation against the Rust

Three levels, cheapest first.

1. **The ported tests.** Same inputs, same assertions.
2. **Fixtures captured from the Rust.** Where a test in Rust asserts on a
   serialised shape (a parcel, a trigger's key, a journal request body), the
   Rust value is captured once into `Armory.Core.Tests/Fixtures/` and the C#
   is asserted equal to it. Captured on the Linux box with `cargo test`; this
   machine has no Rust.
3. **The server.** A C# client and a Rust client against the same
   `armory-server` account must converge: push from one, pull from the other,
   compare `high_water` and row counts. `Armory.SyncCheck` does the C# half.

## This week's addon additions

The Rust being ported is the working tree, which on 2026-09-14 carried
uncommitted additions from the week before: the client `flavour` row and
`Client` record, the Great Vault (`vault`, `vaultReady`), the decor
collection, and three new chronicle happenings (`weather`, `worldtier`,
`wipe`) with `world_tiers` and `weather` on the digest. **None of it touched
`sync::TABLES` or the schema.** They travel inside `detail.json` and
`session.json`, which the server takes whole, so the NAS image from August
carries them without an update. The collector port reads all of the first
three; the chronicle port must read the three happenings.

## Things that will bite

- **`dotnet test` does not run these tests.** The .NET 10 SDK insists on
  Microsoft.Testing.Platform mode for xunit v3 and the documented
  `dotnet.config` opt-in was not honoured from the CLI here. The test project
  builds an executable that *is* the platform, and `test.ps1` runs that.
  `--filter-method "*Name*"` narrows it.
- **The solution is `Armory.slnx`**, the XML format the .NET 10 SDK creates by
  default. Build it without `-p:Platform`: the App is x64-only and the
  libraries are AnyCPU, and a solution-level platform override finds no
  configuration that both fit.
- **A disposed `Store` clears its connection pool.** Microsoft.Data.Sqlite
  pools connections and keeps the file open after the last one is disposed,
  which is exactly the moment a test wants to delete the directory.
- **Migrations are generated code**, exempt from the analysers in
  `.editorconfig`. Regenerate with `dotnet ef migrations add <Name> --project
  Armory.Core --output-dir Store/Migrations`; the design-time factory writes a
  throwaway `design-time.db` beside the project, delete it.
- **EF adds `__EFMigrationsLock` beside `__EFMigrationsHistory`.** The
  schema test exempts anything starting `__EF`, not a fixed list.
- **A selection event fires inside the first measure pass.** A `SelectorBar`
  whose item is created with `IsSelected = true` raises `SelectionChanged`
  while it is being measured, and a handler that clears the page's children
  there kills the process with `0xC000027B` and one line in the event log.
  Every selection or toggle handler compares against the current state and
  returns when nothing changed, and redraws through
  `DispatcherQueue.TryEnqueue`, never inline. The first launch found this on
  two pages.
- **`claude --bare` never finds the login.** Bare mode skips the keychain
  and does not read `CLAUDE_CODE_OAUTH_TOKEN` either, so a print-mode call
  with it answers `Not logged in` beside a terminal that is signed in. It
  would be the natural flag for a scripted caller, and it is the one flag
  `Journal.Compose` must not pass; there is a test saying so. The other way
  to lose the subscription is an `ANTHROPIC_API_KEY` in the environment,
  which print mode uses ahead of the login and bills per token —
  `ClaudeCode.Run` withholds it from the child. Each entry costs the CLI's
  own fixed prefix, twenty-odd thousand input tokens, against the plan's
  window; one evening is nothing, sixty at once is why "Write Every Entry"
  stays strictly one at a time.
- **A Windows app has no stderr anybody sees.** `App.UnhandledException`
  appends to `%LOCALAPPDATA%\Armory\armory.log`; that file is where a stowed
  exception's real message and stack are.
- **`dotnet publish` leaves the compiled XAML behind.** An unpackaged
  WinUI publish copies neither the `.xbf` files nor `Armory.App.pri`, and the
  published window dies in `InitializeComponent` with "Cannot locate resource
  from ms-appx:///MainWindow.xaml". `publish.ps1` copies them from the build
  output; the smoke test of the published build is what found it.
- **`ARMORY_PREVIEW=<dir>` paints every place to PNGs and exits.** The port
  of `examples/preview.rs` (`Armory.App/Shell/Preview.cs`), and the way a UI
  change gets looked at. `ARMORY_PREVIEW_THEME=light` for the other theme;
  `ARMORY_PREVIEW_HOLD=<place|sharing>` leaves the window open on one place
  for a screen capture from outside, because a rendered bitmap holds no popup
  layer and that is where a dialog lives. Sharing is not started under
  preview, so a preview never pushes. **A paint is the viewport**, and the
  window cannot be made taller than the screen (`ARMORY_PREVIEW_HEIGHT`
  asks, the shell clamps at 1613 here), so a page longer than one screen
  needs `ARMORY_PREVIEW_END=1`: a second picture per place, `<place>-<theme>-end.png`,
  scrolled to the foot. That is the only way to see the Run page's goal
  browser, which sits under the road.

  **`ARMORY_PREVIEW_SAMPLE=1` paints the made-up account instead of this
  machine's.** `Armory.Client/Shell/Sample*.cs` is the port of the Rust
  `sample_*` builders — seven characters, three enrolled; a run part-way
  through with goals in every bucket and standing; three evenings (a full
  one, a quiet one, a raid night with dialogue, weather, a world tier and
  wipes), one written up; every collection kind; provenance and inherited
  standings; nine days of market history, recipe books and caged pets.
  `Sample.Seed` writes it through the store's own writers into a store in
  memory, the settings go under `%TEMP%\armory-preview-sample` with a Rarity
  fixture installed beneath them (the odds have no setter; the install path
  is the seam), and the real store is never opened. Two things the pages
  compute rather than read had to be reached differently: the goal rows keep
  no evaluation, so the preview hands `Sample.Dump()` to `Account.Collected`
  and the planner draws the progress bars; and reputations come out of the
  response cache, so the sample stores one Blizzard-shaped body per enrolled
  character. Not reachable without a Battle.net sync: the token price and the
  collectibles-on-sale offers, which only `SyncMarket` fills.

  **Every page is measured against the width the frame gives it** — the port
  of `tests/width.rs`. After each page settles, the element inside its scroll
  viewer is measured at the frame's width and its desired width compared to
  it; `width.txt` in the output directory (and the launching console, when
  there is one) carries one `width ok|OVER <place> <w>/<budget>` line per
  page, and the process exits 1 if any is over. A page that throws while
  painting under preview is logged to `armory.log` and marked handled, so the
  other pages still get their picture; the log is where the finding is read.
  The first sample run found one: `RunPage.Redraw` wraps the long-lived
  `browser` panel in a fresh `Widgets.Card` on every redraw without detaching
  it from the previous card, and the second redraw with a run present dies in
  `Border.set_Child` — any real account with a run hits it on the first
  `Changed` after the page is built.
- **`HasDefaultValue` makes EF omit a column whose CLR value is the default.**
  An insert of `sellable = 0` on `item` is sent without the column, and the
  database default (`1`, "unknown is sellable") wins over the value the code
  wrote. `ItemStore` works around it by writing the row and then setting the
  property; `HasSentinel` on `ItemRow.Sellable` is the proper fix and
  belongs in the next migration.
- **A run's goals travel under `run.key`, never `run.id`.** The replica's row
  lookup went through the primary key at first and the ported
  `a_run_and_its_goals_travel_and_stay_one_run` caught it: two machines pick
  different ids for one run. `Find` now builds its expression over the wire
  key columns for every table.
- **`Region` is `rename_all = "lowercase"` in the Rust.** A copied
  `settings.json` says `"eu"`, and `System.Text.Json`'s enum default would
  have written `"Eu"`; `RegionConverter` writes the code.
- **What the port found that the Rust has not named.** Three small things
  the forks noticed while porting, kept here rather than acted on: the
  citation on a zone's prose and the fact on an item are records with no
  name in the Rust (`Citation`, `ItemFact` here); `Chances.OneIn`'s "a chance
  is one in *n*" reading belongs beside `Collectible`'s link id rather than
  on the page; and the ordering `collapse_toys` leaves is descending item
  id, which the collection page must not rely on.

## What this machine still needs

- **Tailscale**, signed in, so the NAS answers. Connected on 2026-09-14;
  the server answers at `http://nas.example.ts.net:8084`.
- The server's address and the token from `server/.env` on the machine that
  deployed it. They go into the Account & Sharing dialog, never into a file
  in this repo.
- `dotnet-ef` as a global tool, for migrations. Installed by `test.ps1`'s
  first run if missing.
