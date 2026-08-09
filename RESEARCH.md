# Armory — what the data sources will and will not give us

Research notes, 2026-08-03. Facts here decide the shape of the app; the design
follows in `DESIGN.md` once scope is picked.

## The local situation

WoW retail lives at
`~/Games/battlenet/compatdata/pfx/drive_c/Program Files (x86)/World of Warcraft/_retail_`.
Account `PLAYER1`, 23 characters across five realms — Mannoroth (5), Garrosh (1),
Emerald Dream (7), Thrall (4), Dalaran (6). No addons installed, so a companion
addon is a clean slate rather than something that has to coexist.

Five realms means up to five separate connected-realm auction houses. Region-wide
commodity pricing is shared; everything else is not.

## Blizzard Profile API — what it answers

Base `https://{region}.api.blizzard.com`, namespace `profile-us`, OAuth bearer in
the `Authorization` header (query-param tokens stopped working 2024-09-30).

Account scope, needs a user token with `wow.profile`:

| Resource | Path |
| --- | --- |
| Every character on every WoW account | `/profile/user/wow` |
| Account collections | `/profile/user/wow/collections/{mounts,pets,toys,heirlooms,transmogs}` |
| Gold, deaths, position, gold gained/lost | `/profile/user/wow/protected-character/{realmId}-{characterId}` |

Character scope, base
`/profile/wow/character/{realmSlug}/{characterName}` (lowercase name):

`` (summary), `/status`, `/achievements`, `/achievements/statistics`,
`/appearance`, `/collections`, `/collections/{heirlooms,mounts,pets,toys,transmogs}`,
`/encounters`, `/encounters/{dungeons,raids}`, `/equipment`, `/hunter-pets`,
`/character-media`, `/mythic-keystone-profile`,
`/mythic-keystone-profile/season/{id}`, `/professions`, `/pvp-summary`,
`/pvp-bracket/{bracket}`, `/quests`, `/quests/completed`, `/reputations`,
`/soulbinds`, `/specializations`, `/statistics`, `/titles`.

### Achievements carry criteria progress

`/achievements` is not a list of completions. Each entry is

```
{ achievement: {name,id}, completed_timestamp?, id,
  criteria?: { id, amount?, is_completed,
               child_criteria?: [{ id, amount?, is_completed, child_criteria? }] } }
```

An achievement with *any* progress appears, complete or not, and the criteria
tree is recursive with `amount` counters. That is the whole basis of "which
achievements are closest to done" — it is available and nobody much uses it.
`category_progress` and a `recent_events` list come along in the same response.

### Transmog returns appearances, not sources

`/collections/transmogs` gives `appearance_sets` (collected set IDs) and `slots`,
each slot holding collected **appearance IDs** — references into
`/data/wow/item-appearance/{id}`. Blizzard confirmed the omission of the source
item is deliberate: unique appearances, not sources. So the API can say "you have
this look", never "you got it from this item", and never "these three items give
one look you already own". The dedup problem has to be solved against a catalogue.

Added August 2024, so it is newer than most of the third-party tools.

### What the API cannot see, at all

- Bag, bank and **Warband bank** contents. No endpoint, and Blizzard has said one
  is not planned.
- Currencies. Nothing — no Trader's Tender, no Valorstones, no crests.
- Mail, guild bank, keybinds.
- Unlearned recipes. `/professions` lists `known_recipes` only.
- Anything live. **Profile data is a snapshot written when the character logs
  out.** Blizzard staff, plainly: profile data changes only on logout. Every
  endpoint sets `Last-Modified` and honours `If-Modified-Since`, which is how you
  cheaply detect that a character has actually played since last sync.
- **Anything sequential.** This one is worth stating separately, because it is
  not a missing endpoint so much as a missing *shape*. The profile API is a set
  of current values with no history attached to any of them. `/quests/completed`
  is a list of ids with no dates; `/achievements` carries one completion
  timestamp per achievement and none for the criteria under it; nothing
  anywhere records a zone entered, a death, a wipe, a loot roll or an order of
  events. A tool that wants to say "here is what you did on Tuesday" cannot get
  it from Blizzard at any price. It has to be recorded while it happens, which
  means an addon.

Gold is the exception to the "not available" list — it is on the
`protected-character` endpoint, along with `total_gold_gained`/`lost` and death
counts.

Reputation has no account-scoped endpoint, but The War Within made most reps
account-wide server-side, so the per-character `/reputations` response already
returns the shared Warband value.

## OAuth — the constraint that shapes onboarding

**Battle.net does not support PKCE.** A Blizzard developer-relations reply
(Oct 2024) states every registered client is a *confidential* client; there is no
public/native client mode. The client secret is mandatory for the authorization
code flow, and shipping it in a binary is both a ToS violation and a shared
rate-limit pool.

- `http://127.0.0.1:PORT/callback` **is** an accepted redirect URI. Blizzard's own
  sample uses `http://localhost:8080`. It must be an exact registered string, so
  the port is fixed, not ephemeral.
- Custom schemes (`armory://callback`) are rejected — must be http/https.
- Limits are 100 req/s and 36,000 req/hour, counted **per client_id**, not per IP.
- Portal moved to `community.developer.battle.net` and is flaky: 500s on client
  creation reported continuously from Nov 2025 into mid-2026. Client names must be
  globally unique and the UI does not say so, which is one cause. Onboarding has to
  tolerate this.

So: the user registers their own client and pastes the id and secret in. This is
what every FOSS tool in this space does, it is what Blizzard's own quick-start
tells a developer to do, and it hands each user their own 36k/hour quota instead
of sharing ours. Same shape as Sleeve's Discogs token.

**ToS obligations that are actually design constraints:** a mandatory 30-day TTL
on all data obtained through the API — cached data must be refreshed or dropped
inside 30 days. Distributing to the end user through the app is licensed;
redistributing bulk data is not. Sharing API keys is forbidden. An older policy
document bans paywalling API-powered features.

## Auction house

| Purpose | Endpoint | Namespace |
| --- | --- | --- |
| Connected realm index / detail | `/data/wow/connected-realm/index`, `/{id}` | `dynamic-us` |
| Realm auctions (gear, pets, BoE, recipes) | `/data/wow/connected-realm/{id}/auctions` | `dynamic-us` |
| Region commodities | `/data/wow/auctions/commodities` | `dynamic-us` |
| WoW Token | `/data/wow/token/index` | `dynamic-us` |
| Item search / media / classes | `/data/wow/search/item`, `/data/wow/media/item/{id}` | `static-us` |

Snapshots refresh hourly, all realms in a region landing within the same minute or
two. `Last-Modified` + `If-Modified-Since` → 304 works everywhere **except
commodities**, which costs 25x quota per call and is charged that even for a 304.

Realm auction records carry `bid`, `buyout`, `quantity`, `time_left`, and an
`item` with `bonus_lists`, `modifiers`, `context`, and pet fields
(`pet_species_id`, `pet_breed_id`, `pet_level`, `pet_quality_id`). Commodities
carry only `unit_price` and `quantity` — no bonus IDs, because commodities have no
variance. Blizzard publishes no dictionary for bonus IDs, modifier types or
context values; every tool reconstructs them.

There is **no sale signal**. Quantity disappears between snapshots and you infer
sales by diffing.

Sizes: a connected realm's non-commodity file has run 26–28 MB since the 2022
cross-faction merge. Five realms hourly is ~3 GB/day raw. Undermine Exchange
stores per-item deltas rather than snapshots and holds 186 realms in ~56 GB;
delta storage is not an optimisation here, it is the only viable approach.

Blizzard offers **no price history**. Third parties: Undermine Exchange (alive,
free tier behind a Patreon-linked key since Feb 2026), TSM's public CSV endpoint
(no key), Saddlebag Exchange (alive). Classic AH endpoints have been 404ing since
at least April 2026 with no fix — Classic is not a target.

## The addon route

An addon cannot open a socket or read a file. The sandbox strips `io`, `os`,
`debug`, `require`, `loadfile`. The only channel out of the game is
**SavedVariables**, written at logout or `/reload`, as plain Lua source at
`_retail_/WTF/Account/PLAYER1/SavedVariables/<Addon>.lua` and
`.../PLAYER1/<Realm>/<Character>/SavedVariables/<Addon>.lua`. The only channel in is
writing a Lua file there before the game loads. There is a Lua VM ceiling of
262,144 unique literal values per file.

What that buys, none of which the web API has:

| Data | API |
| --- | --- |
| Bags, bank, reagent bank | `C_Container.GetContainerItemInfo`, `Enum.BagIndex` |
| **Warband bank** | `C_Bank.*` (11.0), `Enum.BankType.Account`, `C_Bank.FetchDepositedMoney` |
| Currencies | `C_CurrencyInfo.GetCurrencyInfo` — incl. `isAccountWide`, `isAccountTransferable` |
| Transmog per-source state | `C_TransmogCollection.GetAppearanceSources(id)` → `.isCollected` per source |
| Profession recipes & knowledge | `C_TradeSkillUI.*` |
| Completed quests in bulk | `C_QuestLog.GetAllCompletedQuestIDs()` |
| Full AH scan | `C_AuctionHouse.ReplicateItems()` — throttled to once per 15 min |

`GetAppearanceSources` is documented by addon authors as not always returning
every source for an appearance, so it improves the dedup problem without closing
it.

### What an addon can see happening, which the API cannot see at all

The table above is state the API omits. This is *events*, which the API has no
shape for in the first place — the whole basis of the chronicle:

| Happening | Event, and what it carries |
| --- | --- |
| Where the character is | `ZONE_CHANGED`, `ZONE_CHANGED_INDOORS`, `ZONE_CHANGED_NEW_AREA` → `GetZoneText`, `GetSubZoneText`, `GetRealZoneText` |
| A quest accepted | `QUEST_DETAIL` → `GetTitleText`, **`GetQuestText`** |
| A quest turned in | `QUEST_COMPLETE` → `GetTitleText`, **`GetRewardText`**, `GetObjectiveText`; then `QUEST_TURNED_IN(questID, xp, money)` |
| A level | `PLAYER_LEVEL_UP(level, …)` |
| A death | `PLAYER_DEAD` |
| A boss | `BOSS_KILL(encounterID, name)`; `ENCOUNTER_END(encounterID, name, difficultyID, groupSize, success)` — the *wipes* too |
| Loot worth mentioning | `CHAT_MSG_LOOT` → the `|Hitem:<id>` inside the link → `C_Item.GetItemInfo` for name and quality |
| An auction sale | `MAIL_INBOX_UPDATE` → `GetInboxHeaderInfo(i)` → `sender`, `subject`, `money` |
| An achievement | `ACHIEVEMENT_EARNED(id)` → `GetAchievementInfo` |
| Something collected | `NEW_MOUNT_ADDED`, `NEW_PET_ADDED`, `NEW_TOY_ADDED` |
| Who you played with | `GROUP_ROSTER_UPDATE` → `UnitName("partyN")` |

**`GetQuestText` and `GetRewardText` are the find here.** They are the prose the
player just read — the premise, and what was said at the turn-in. Nothing in the
web API returns quest text of any kind, and the two of them together mean a tool
can know what an evening was *about* without fetching a word from anywhere else.
Both are only readable while the quest frame is open, so the turn-in text has to
be captured at `QUEST_COMPLETE` and attached at `QUEST_TURNED_IN`, which is the
event that carries the id.

Two traps, both learned from the collector:

- **At `PLAYER_LOGOUT`, `GetMoney` and `GetAverageItemLevel` answer 0.** A
  session that assigns its closing figures there records an evening that cost
  the character everything they had. Track them as they change and never replace
  a real number with a zero.
- **WoW's serializer writes a table with an interior `nil` as keyed entries
  rather than as a padded array.** `{ at, "quest", 123, nil, "text" }` and
  `{ at, "quest", 123, "title", "text" }` therefore come back in two different
  shapes. Writing `""` for absent fields keeps every row dense and gives the
  reader one shape.

Addon policy permits this. The 2018 UI policy requires free, unobfuscated,
publicly visible code and no ads; the EULA bans automation of gameplay. A
data-capture addon is what TSM, Auctionator, Altoholic and WoWthing Collector have
done unchallenged for years. The Nov 2025 "addon disarmament" work targets
real-time combat decision-making and does not touch this. The Developer API ToS —
including the 30-day TTL — does not apply to data that came through
SavedVariables, because that path never touches Blizzard's web API.

Precedent for the whole pattern: **WoWthing Collector** (addon writes, desktop
clients file-watch `WTF/` and upload on logout — `wowthing-sync`, `wowthing-sxnc`)
and the **TSM Desktop App** (writes `AppData.lua` into SavedVariables before
login; reverse-engineered as `exceptionptr/tsm-app-linux`, PySide6, which exists
precisely because the official one will not run under Wine).

## "Where do I get this" — the catalogue problem

Blizzard's ceiling is low. `/data/wow/mount/{id}` has a `source` object with a
coarse `type` (`VENDOR`, `DROP`, `QUEST`, …) and a generic name — no NPC, zone,
coordinates, drop rate or lockout, and many mounts have no source at all. Pets
have no structured source field. Toys are not a first-class type. Achievements
give the criteria tree, which is plumbing, not instructions. There is no loot
table endpoint anywhere.

| Source | Has | Format | Licence | Viability |
| --- | --- | --- | --- | --- |
| **AllTheThings** | quest chains, vendors, drop hierarchy, crafting, achievement trees — the real "how to obtain" logic | Lua tables under `db/{Expansion}/` | MIT | Best single source. No stable schema; expect drift per patch |
| **Rarity** | per-item estimated drop rates, NPC detection lists | Lua tables | GPLv2 | Best legally-obtainable bulk drop-rate data |
| **wago.tools** | full DB2 client tables — `Mount`, `Achievement`, `CriteriaTree`, `BattlePetSpecies`, `ItemAppearance`, `TransmogSet` | CSV per table, per build | none published | Excellent catalogue; **no loot tables — that data is server-side and never ships to the client** |
| TrinityCore world DB | real loot `Chance` values | SQL | GPLv2 | Good through Wrath, unreliable for current retail |
| Warcraft Logs / Raider.IO | parses, M+ scores | GraphQL / REST | free tiers | No sourcing data |
| **Wowhead** | the best data in the ecosystem | HTML | ToS forbids automated access; robots.txt names `ClaudeBot` and `anthropic-ai` in its Disallow list | Off the table |
| **warcraft.wiki.gg** | the best *lore* text in the ecosystem, CC-licensed | MediaWiki | CC BY-SA; robots.txt disallows `/api.php` for crawlers, which is a directive to *crawlers* | Not needed — see below |

The wiki turned out not to be needed at all, which is a better outcome than
being allowed to use it. The addon's `GetQuestText` and `GetRewardText` are the
game's own words about the exact quests the player did, and
`C_CampaignInfo.GetCampaignInfo` adds Blizzard's own paragraph about the
storyline they belong to. First-party text, about this evening specifically, at
no request cost. No summary of it is an improvement.

Two things still hold regardless of who is allowed to fetch what: request
volume should stay polite (the wiki asks this directly in its own robots.txt
comments), and CC BY-SA attribution has to travel with any text that is
displayed. Armory links rather than fetching, so neither currently applies.

Icons come from `/data/wow/media/{type}/{id}`, which returns a hotlinkable
`render-us.worldofwarcraft.com` URL. That is the sanctioned path.

## The soft reset — replaying content an account already remembers

The stated goal is to go and earn things again. The account is the obstacle: a
decade of account-wide achievements and account-wide collections means the game
itself will not present most of this content as new. Three tiers, and the
distinction is the whole feature:

**Genuinely repeatable per character.** Quest completion is per character —
`/quests/completed` and `C_QuestLog.GetAllCompletedQuestIDs()` both answer for one
character only, so questlines, Loremaster-style content and zone storylines reset
on a new character. Statistics are per character. Dungeon and raid clears are per
character. Professions are per character. Achievements whose criteria are
explicitly per-character behave the same way.

**Permanently spent.** Account-wide collections — a mount, pet or toy the account
owns cannot be collected again, and many bind-on-pickup mounts will not drop at
all for a character whose account already has them. Feats of Strength. Removed
content. No tool can give these back, and the honest thing is to mark them so a
plan is not built on them.

**Contaminated by Warbands.** The War Within moved most reputations to
account-wide, syncing to the furthest-progressed character. Renown, most
Dragonflight and War Within reps, and a growing share of currencies are now shared.
So the axis a soft reset most naturally runs along — "grind this faction again" —
is the axis Blizzard has spent two expansions removing. This needs to be reported,
not discovered halfway through.

The consequence for the app: **Blizzard's completion state cannot be the app's
completion state.** The account says done; the run has not done it. Armory has to
own a second ledger, seeded from a baseline snapshot taken on a chosen date, and
show progress against the run rather than against the account.

Two things make that ledger possible:

- **`GetAchievementInfo(achievementID)` returns `wasEarnedByMe` and `earnedBy`** —
  the character who actually earned each account-wide achievement. The web API has
  no equivalent field; this is addon-only, and across 23 characters it is the
  difference between "the account has this" and "Aeltor has this, you are on
  Somechar". *Needs confirming in-game; not verified in this pass.*
- Collection diffs plus loot events give a per-run acquisition log for everything
  the game itself will not distinguish.

A genuinely clean reset means a separate Battle.net account, not a separate
character or license — `/profile/user/wow` returns `wow_accounts` (plural) because
one login can hold several WoW licenses, but collections are shared across them.
So the app should be able to hold more than one Battle.net account and compare
them.

## Where the gap actually is

Every existing tool optimises for one character, one session, one data category.
The moment reasoning has to span characters, time, realms, items or content types,
the ecosystem falls back to an overwhelming everything-addon or a spreadsheet.

- **"Which alt should I play next" is unsolved.** The `wow-weekly` addon exists
  because its author could not find one; it reasons only about gear upgrades.
  Competing Great Vault checklists number at least six, each missing what the
  others have — six authors independently rebuilt the same grid rather than
  collaborate.
- **Weekly routing exists only for Mythic+**, and only as route drawing.
  Nothing sequences collection goals across a roster.
- **Cross-realm arbitrage is technically solved but not humanely** — Saddlebag
  Exchange is built by and for goldmakers who think in marketshare deltas.
- **Appearance dedup is punted on by everyone including Blizzard's own API.**
  ATT attempts it and its own community warns newcomers off it as overload.
- **Achievement dependency chains have no equivalent of BtWQuests.** Nothing says
  "this meta needs two holiday achievements that are eleven months away".
- TSM is a $3–7.50/month subscription that draws genuine hostility, and its
  desktop app does not run on Linux.

**Nothing in this space is a native Linux app.** Addon managers are Clojure/JavaFX
(Strongbox), Python (instawow), Tauri (Grimoire — which explicitly declines to
support Linux), Electron (Innkeeper), PySide6 (tsm-app-linux). No GTK, no
libadwaita, anywhere. That is an open lane, not a crowded one.

## The comparable tools, and what is worth taking

Reviewed August 2026: WoWthing, Data for Azeroth, Wowchievement, WoWProgress.
All four are websites; none is a desktop application and none runs offline.

**WoWthing** is the deepest of them and the closest neighbour. Character grid,
currencies, professions with knowledge points, transmog sets, reputations,
lockouts, weekly vault, a HandyNotes-style map, and a cross-tabulating "matrix".
It is explicit that most of that needs its own collector addon uploaded by hand.
Its best idea by a distance is **auction sourcing**: it knows what the account
has not collected and finds it listed on the auction house, cheapest realm
first. That is a join of two things Armory already holds, and it is now
`model::market::on_sale`.

**Data for Azeroth** is leaderboards and rarity — what fraction of players own
each mount. The rarity figure is its own aggregate over its own users, not
anything Blizzard publishes, so Armory cannot have it: fetching it would be
scraping a third party, which is the same line Wowhead sits behind. Its
**completion score** is a published formula over data we already hold (100
points a mount, toy, title, exalted reputation; 3 per achievement point; 1 per
appearance) and could be computed locally if a single headline number is ever
wanted.

**Wowchievement** is renown and event tracking, entirely from the web API with
no addon at all — completed quest ids, achievements and reputations, joined
against a hand-curated list of what each activity awards. The curation is the
whole product; the API half is what Armory already fetches.

**WoWProgress** is rankings and recruitment, which needs a global database of
every guild. Nothing there transfers to a single-account tool except the shape
of its raid-progress view, which is per-tier per-difficulty boss kills — and
`/profile/wow/character/{r}/{n}/encounters/raids` answers exactly that. Armory
currently flattens that response to a set of encounter ids for criteria and
throws the structure away. Reconstructing it is the cheapest unbuilt page here.

## Housing

Housing shipped with Midnight, and Blizzard released REST endpoints for it on
2025-12-19 — unusually promptly, and specifically for websites rather than
addons. This is the one major system where **the API is ahead of the addon**.

| Endpoint | Namespace | Gives |
| --- | --- | --- |
| `/data/wow/decor/index` | static | Every piece of decor: id and name |
| `/data/wow/decor/{id}` | static | One piece, with its item and source |
| `/data/wow/search/decor` | static | Search |
| `/profile/user/wow/collections/decor` | profile | What the account owns, with counts |
| `/profile/wow/character/{r}/{n}/collections/decor` | profile | The same, per character |
| `/profile/wow/character/{r}/{n}/house/{houseId}` | profile | A house's placed objects |
| `/data/wow/room/*`, `/data/wow/fixture/*` | static | Rooms and fixtures |
| `/data/wow/neighborhood-map/*` | dynamic | Neighbourhoods |

So decor is a collection with a catalogue and an owned set, which is the same
shape as mounts, and it is a fourth `Kind` rather than a subsystem.

Two things it is not. It is **not** a house editor: the house endpoint returns
placed object transforms, several community tools already render them, and
Blizzard's own forum thread is openly uneasy about the privacy of that. And
**quantity is dropped** — decor is owned in counts, because a room wants six of
the same chair, and Armory records owned or not owned because it tracks a
collection rather than furnishes a house.

The client-side `C_HousingCatalog` is richer than the web API — it carries
`sourceText`, the same sentence the mount and pet journals give, plus placement
costs and category tags. That is the same asymmetry as everywhere else and the
addon could close it, but housing is the one collection where the web API alone
produces a usable page.

## What needs an addon, and what does not

| Capability | Web API | Collector addon |
| --- | --- | --- |
| Roster, levels, classes, races | Yes | Yes, plus characters never logged out |
| Item level, spec, professions, keystone rating | Yes | — |
| Gold | Yes, one call per character | Yes, all at once |
| Mounts, pets, toys — owned and catalogue | Yes | Yes |
| **Where a collectible comes from, in a sentence** | No — one word or nothing | **Yes** |
| Collectible artwork | Renders are addressable from the display id the addon supplies | Supplies the display id |
| Housing decor — owned and catalogue | **Yes** | Not read |
| Achievements: completion and criteria trees | Yes, account-wide | Yes, plus a flat tree |
| **Which character earned an account-wide achievement** | **No — no field exists** | **Yes (`earnedBy`)** |
| **What a criterion measures** | No — structure without meaning | **Yes** |
| Reputations and renown | Yes, per character | Yes |
| Completed quests, statistics, encounters | Yes, per character | Quests only |
| **Currencies** | **No endpoint** | **Yes** |
| **Warband bank** | **No endpoint, and none planned** | **Yes** |
| **Weekly vault, raid lockouts** | No | Would need one; unbuilt |
| Auction prices and the token | Yes | — |
| Realm and connected-realm lists | Yes | — |

The short version: **Blizzard answers "what does this account have"; the addon
answers "what does it mean and who did it"** — with housing as the exception
that proves it, being a collection Blizzard exposes and the client does not
write anywhere Armory can read.
