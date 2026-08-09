-- Armory Chronicle
--
-- Records what happened during a play session, so the Armory desktop
-- application can turn it into a journal entry. Capture only, like the rest of
-- this addon: it reads documented events and writes one table.
--
-- The collector beside this file answers "what does the account have". This
-- answers "what did this character do on Tuesday", which is a different
-- question and needs a different shape — a *sequence*, not a set. Nothing in
-- the web API can answer it at all. Blizzard's profile data is a logout
-- snapshot with no history in it: it will say a character has 4,312 completed
-- quests and never which twelve of them were finished this evening, never
-- which zone they were standing in, and never that they died twice to the same
-- rare.
--
-- The one thing here worth more than every other field put together is the
-- **quest text**. `GetQuestText` and `GetRewardText` are the sentences the
-- player actually read at the turn-in — the story as the game told it. No
-- endpoint returns them, and they are the difference between a journal entry
-- that says "completed 12 quests" and one that says what the quests were
-- about. Capturing them here is also why Armory fetches nothing from a wiki:
-- the evening's lore was already on the screen, and the game hands it over for
-- free.
--
-- Per character, deliberately. A session is a sequence of events, and merging
-- twenty-three characters' worth into the account file would run at the Lua
-- constant-table ceiling the collector already has to work around.

local ADDON = ...

--- Bumped when the shape of the saved table changes.
---
--- 2 added campaigns, what killed you, instances and keystones, named rares,
--- scenarios, skill-ups, gear upgrades, appearances, and the session-level
--- kill count and reputation ranks.
--- 3 added the money ledger, crafting, flights, distance travelled, the
--- longest fight and the worst hit taken.
local FORMAT = 3

--- How many sessions to keep, oldest dropped first.
---
--- The desktop application copies each one into SQLite the first time it sees
--- it and never needs it again, so this only has to survive the gap between
--- playing and next launching Armory. Forty is a couple of months of evenings.
local MAX_SESSIONS = 40

--- How many events one session may hold.
---
--- A long raid night is a few hundred. The cap is what stops an overnight AFK
--- in a busy city writing a file nobody can load.
local MAX_EVENTS = 500

--- Loot below this quality is not a story. 3 is Rare (blue).
---
--- Every grey and green picked up in an evening is several hundred lines that
--- say nothing, and they would drown the dozen that matter.
local LOOT_QUALITY = 3

--- How many overheard lines one session may keep.
---
--- Its own budget rather than a share of `MAX_EVENTS`, because ambient chatter
--- is the one thing here that arrives faster than anything else and would
--- otherwise crowd out the quests, the deaths and the loot — the events an
--- evening is actually about.
local MAX_SAID = 40

--- How long an overheard line may be, in bytes. Cut like quest text is.
local SAID_TEXT = 240

--- How many gossip lines one session may keep.
---
--- Smaller than `MAX_SAID`, because the hit rate is worse: a great deal of
--- gossip is "Greetings, champion" from somebody selling reagents. It is kept
--- anyway — the judgement about which of it is worth a sentence belongs to the
--- model reading the log, not to a length heuristic guessing in the addon.
local MAX_GOSSIP = 25

--- How much of a quest's text to keep, in bytes.
---
--- The premise is in the first couple of sentences; the rest is usually
--- directions to a cave. A dozen turn-ins an evening at full length would be
--- most of the file.
local QUEST_TEXT = 400

local frame = CreateFrame("Frame")

--- The session being recorded, or nil between logout and the next login.
local current = nil

--- Quest text captured at the reward frame, held until the turn-in that
--- follows it. The frame shows one quest at a time, so there is nothing to key
--- this by.
local pending = nil

-- Everything below is state for the session currently being recorded, declared
-- here rather than beside the handler that uses it because `openSession` has to
-- clear all of it in one place. A value left over from the last evening is a
-- fact reported about the wrong one.

--- Mail already recorded, so re-opening the mailbox does not record a sale
--- twice. Mail has no id an addon can read, so this is keyed on content.
local seenMail = {}

--- Campaigns already named this session, so a chapter each does not repeat it.
local seenCampaign = {}

--- Lines already overheard, so the guard who shouts the same thing every ninety
--- seconds is one memory rather than forty.
local seenSaid = {}
local saidCount = 0

--- The same, for what NPCs say when you click them. Its own set and its own
--- budget: most of it is a vendor greeting, and a shared budget would let those
--- crowd out the dialogue an evening is actually about.
local seenGossip = {}
local gossipCount = 0

--- Where the character was last seen, so a route is a route and not a list of
--- every time the game re-fired the zone event. `lastMap` is the `UiMapID`,
--- which is what anything joining a session to a place keys on — a zone name
--- is not unique.
local lastZone, lastSubzone, lastMap

--- Which instance, at which difficulty, was last recorded.
local lastInstance = nil

--- The last thing to damage the player, held until they either die or do not.
local killedBy = nil

--- How many things the party finished off.
local kills = 0

--- The player's own GUID, read once.
---
--- The combat log fires hundreds of times a second in a busy fight and every
--- one of them is compared against this. Asking the game for it each time is
--- the difference between a handler that costs nothing and one somebody
--- notices.
local playerGUID = nil

--- Profession ranks and equipped item levels as they were last seen, so that
--- "went up" can be told from "was already that".
local skills = {}
local equipped = {}

--- What the character is standing in front of, for the money ledger.
---
--- `PLAYER_MONEY` says the total changed and never why. Everything that takes
--- or gives money in this game does it through a frame — a vendor, the auction
--- house, the mailbox, a flight master — so what the frame *is* answers the
--- question the event will not. Nothing else here needs a state machine; this
--- does, because "spent 400 gold" and "spent 400 gold at the auction house"
--- are different facts and only one of them is worth writing down.
local context = nil

--- The money total as of the last event, so a delta can be taken.
local purse = 0

--- What repairing everything would cost, sampled while a merchant is open.
--- A repair and a purchase are both money leaving at a vendor, and this is what
--- tells them apart.
local repairCost = 0

--- When something was last listed at the auction house.
---
--- Money leaving at an auction house is a bid or a listing deposit, and the
--- only thing that distinguishes them is whether an auction was just created.
local listedAt = 0

--- Set when a quest is about to pay, so the money that follows is attributed to
--- the quest rather than to the ground.
local questPaid = false

--- Money seen in the mailbox, by amount, and what it is.
---
--- Gold arriving through a mailbox is three different facts. The auction house
--- sent it, which is income; another character on this account sent it, which
--- is a transfer and is not income at all; or somebody else did, which is a
--- gift. Nothing but the sender's name separates them, and the sender is
--- readable while the inbox is open and gone by the time `PLAYER_MONEY`
--- arrives — so the classification is worked out during the scan and looked up
--- by amount when the money actually lands.
local mailMoney = {}

--- People already counted this session, so being in a party for four hours
--- counts once rather than once per roster update.
local seenParty = {}

--- Where the character has been standing since, so a zone gets credited with
--- the time actually spent in it.
local zoneSince = 0

--- Yards covered, and by what means, since the last flush to the account file.
---
--- Reset by every flush, which is why the session's own total is accumulated
--- separately below: a four-hour evening flushes a dozen times, and reading
--- these at logout would report the last four minutes of it.
local walked = 0
local flown = 0

--- Yards covered this session, which nothing resets until the next login.
local travelled = 0

--- Where the character was at the last sample, in world coordinates.
local lastX, lastY, lastContinent = nil, nil, nil

--- When the fight the character is in now started, and the longest one yet.
local fightSince = 0
local longestFight = 0

--- The hardest single hit taken this session, and the lowest the health bar got
--- without the character dying.
local worstHit, worstHitBy = 0, nil
local lowestHealth = 100

--- How much of the combat log actually arrived, and what it looked like.
---
--- Diagnostic, and here because an evening of twenty kills and a death came
--- back with `kills = 0`, `worstHit = 0` and `lowestHealth = 100` while every
--- other kind of event recorded normally. Everything those three depend on is
--- one handler, so the question is which step of it is silent: the event not
--- arriving at all, no killing blow being attributed, or no damage landing on
--- a GUID that matches the player's. Counting each separately answers it in
--- one evening instead of by reading the code again.
local cleuSeen, cleuKills, cleuHits, cleuMine = 0, 0, 0, 0

--- Forward declaration, for the same reason as `noteInstance` below it:
--- `noteZone` credits the zone being left, and the function that does it is
--- defined further down beside the rest of the session bookkeeping.
local creditZone

--- Forward declaration, same knot: `closeSession` writes the last of the
--- distance out and the sampler that accumulates it is defined below.
local flushDistance

--- Forward declaration. `openSession` records the instance the character
--- logged into, and the function that does it needs `note`, which needs
--- `current`, which `openSession` is the thing that sets. Declaring it here and
--- assigning it below is what unties that knot — a `local function` further
--- down would be a different local and `openSession` would capture nil.
local noteInstance

--- Who we are, spelled the way Armory reads it back. The same function as the
--- collector's beside it; each file is loaded on its own and neither can see
--- the other's locals.
local function whoami()
	local name = UnitName("player")
	local realm = GetRealmName()
	if not name or not realm then
		return nil
	end
	return name .. "-" .. realm
end

--- Add to a lifetime counter in the account-wide file.
---
--- These are the numbers no Blizzard system keeps at any granularity worth
--- having: how many times a recipe has been made, how many evenings somebody
--- has been in the party, how many hours a zone has had, what keeps killing
--- this character. They go in the account file rather than the session so that
--- Armory sees every character's without opening twenty-three files, and they
--- are cumulative rather than per-session because that is the only form in
--- which they mean anything.
---
--- One table for all of them — `tally[character][kind][key] = { count, label }`
--- — rather than one table per kind. The reader is `model::tally`.
local function tally(kind, key, count, label)
	local me = whoami()
	if not me or not key or key == "" or not count or count <= 0 then
		return
	end
	ArmoryCollectorDB = ArmoryCollectorDB or {}
	local store = ArmoryCollectorDB
	store.tally = store.tally or {}
	store.tally[me] = store.tally[me] or {}
	store.tally[me][kind] = store.tally[me][kind] or {}
	local held = store.tally[me][kind][key] or { 0, label or tostring(key) }
	held[1] = held[1] + count
	held[2] = label or held[2]
	store.tally[me][kind][key] = held
end

local function db()
	ArmoryChronicleDB = ArmoryChronicleDB or {}
	local store = ArmoryChronicleDB
	store.format = FORMAT
	store.sessions = store.sessions or {}
	return store
end

--- Seconds since this session started.
---
--- Relative rather than absolute, because it is what an entry gets written
--- from — "an hour in" is a fact about the evening and 1785003600 is not — and
--- because a whole session's timestamps then cost one number rather than one
--- per event.
local function elapsed()
	if not current then
		return 0
	end
	return time() - current.startedAt
end

--- Record one thing that happened.
---
--- Every event is the same five positions: `{ at, kind, a, b, c }`. The reader
--- is ours and the shape is fixed in this file, so named keys would multiply
--- the file's size for no reader that wants them.
---
--- Absent fields are written as empty strings rather than left nil, and that is
--- not tidiness. WoW's serializer writes a table with a hole in the middle as
--- keyed entries rather than as a padded array, so `{ at, "quest", 123, nil,
--- "text" }` and `{ at, "quest", 123, "title", "text" }` come back in two
--- different shapes. Keeping every row dense means one shape to read.
local function note(kind, a, b, c)
	if not current or #current.events >= MAX_EVENTS then
		return
	end
	current.events[#current.events + 1] = {
		elapsed(),
		kind,
		a or "",
		b or "",
		c or "",
	}
end

--- When the last automatic screenshot was taken.
---
--- Levelling in a dungeon can fire three of these within a second — the level,
--- the boss, and the achievement that came with it — and three near-identical
--- pictures is not three memories.
local lastShot = 0

--- The shortest gap between automatic screenshots, in seconds.
local SHOT_GAP = 20

--- Note a moment worth a picture.
---
--- It does not take one. An addon cannot: `Screenshot()` wants a hardware
--- event behind it and there is none inside an event handler, so asking put
--- the client's "blocked from an action only available to the Blizzard UI"
--- dialog on screen at every notable moment instead.
---
--- What is left still works. Armory matches these moments against the
--- modification times of files in the client's own Screenshots folder, so a
--- picture *you* take when something happens is attached to that evening.
--- There is no way for an addon to learn a filename either way, which is why
--- the correlation was ever by time.
local function capture(what, subject)
	local at = time()
	if at - lastShot < SHOT_GAP then
		return
	end
	lastShot = at
	-- **The addon does not press the shutter.**
	--
	-- `Screenshot()` needs a hardware event behind it — a real keypress or
	-- click. Called from a timer, as this was, there is none in the call
	-- stack, and the client answers with the "blocked from an action only
	-- available to the Blizzard UI" dialog. Every notable moment then put a
	-- popup on screen instead of a picture in the journal.
	--
	-- So the moment is recorded and nothing is taken. `Digest::pictures`
	-- matches these against the modification times of files in the
	-- Screenshots folder, which still works — it just means the picture has
	-- to be one you took. Press Print Screen when something happens and it is
	-- attached to that evening.
	note("shot", what, subject)
end

--- Where money went, or came from.
---
--- One event per movement, with the context that explains it. The classifying
--- — which of these count as income, which as a cost, and which are neither —
--- happens in the desktop application, where the reasoning can be written down
--- and tested. The addon says what it saw.
local function noteCoin()
	local now = GetMoney() or 0
	local delta = now - purse
	purse = now

	if delta == 0 or not current then
		return
	end

	-- A quest reward is already recorded, with the quest it came from. Noting
	-- it again here would count the same gold twice.
	if questPaid and delta > 0 then
		questPaid = false
		return
	end

	local where = context
	if not where then
		-- No frame open. Money appearing is something dropping it; money
		-- leaving with nothing to spend it on is rare enough to admit rather
		-- than guess at.
		where = delta > 0 and "loot" or "unknown"
	elseif where == "vendor" and delta < 0 then
		-- A repair and a purchase are both money leaving a merchant. The
		-- repair bill dropping by about what was spent is what separates them.
		local after = GetRepairAllCost() or 0
		local repaired = repairCost - after
		if repaired > 0 and math.abs(repaired - (-delta)) <= math.max(100, (-delta) * 0.02) then
			where = "repair"
		end
		repairCost = after
	elseif where == "auction" and delta < 0 then
		-- A deposit follows a listing within a moment; anything else is a bid.
		where = (time() - listedAt <= 5) and "deposit" or "bid"
	elseif where == "mail" and delta > 0 then
		-- Worked out while the inbox was readable. Anything not recognised
		-- there stays "mail", which is honest: money came out of a mailbox and
		-- we do not know whose it was.
		where = mailMoney[delta] or "mail"
		mailMoney[delta] = nil
	end

	note("coin", where, math.abs(delta), delta > 0 and 1 or 0)
end

--- Open or close a money context.
---
--- Closing takes a final reading: the frame going away is the last chance to
--- attribute what happened in front of it, and a mail taken as the window
--- closes would otherwise land in the ledger as loot.
local function enter(what)
	noteCoin()
	context = what
	if what == "vendor" then
		repairCost = GetRepairAllCost() or 0
	end
end

local function leave()
	noteCoin()
	context = nil
	repairCost = 0
end

-- Distance --------------------------------------------------------------------

--- How often the character's position is sampled, in seconds.
---
--- There is no event for moving. One second is fine enough that a flight path
--- is a curve rather than three straight lines and coarse enough that the
--- handler costs nothing — a character at full speed covers about twenty
--- yards in it.
local STEP = 1

--- The furthest a sample may be from the one before it, in yards.
---
--- Anything beyond this is a portal, a hearthstone, a summon or a loading
--- screen, and counting it would credit somebody with walking to Outland.
--- Nothing in the game moves faster than about 200 yards a second.
local TELEPORT = 400

local sampledAt = 0

--- Write the distance so far into the account file and start again from zero.
---
--- Batched rather than written per sample, because a tally is a table write and
--- doing one every second for an evening is the sort of thing somebody notices
--- in a raid.
function flushDistance()
	if walked >= 1 then
		tally("distance", "foot", math.floor(walked), "On foot")
		walked = walked - math.floor(walked)
	end
	if flown >= 1 then
		tally("distance", "flight", math.floor(flown), "By flight path")
		flown = flown - math.floor(flown)
	end
end

--- How far the character has moved since the last sample.
---
--- `GetWorldPosFromMapPos` is what makes this a distance rather than a
--- fraction: map coordinates are 0–1 across whatever map you are on, so the
--- same 0.01 is a hundred yards in a starting zone and half a mile in Pandaria.
--- The world position is in yards on the continent, which is comparable with
--- itself.
local function sampleDistance()
	if not current or not C_Map then
		return
	end
	local now = time()
	if now - sampledAt < STEP then
		return
	end
	sampledAt = now

	local map = C_Map.GetBestMapForUnit("player")
	local position = map and C_Map.GetPlayerMapPosition(map, "player")
	if not position then
		return
	end
	local continent, world = C_Map.GetWorldPosFromMapPos(map, position)
	if not world then
		return
	end
	local x, y = world:GetXY()
	if not x or not y then
		return
	end

	-- Only compare against a sample from the same continent. Across one, the
	-- coordinate systems are unrelated and the difference is a number with no
	-- meaning rather than a large distance.
	if lastContinent == continent and lastX then
		local moved = math.sqrt((x - lastX) ^ 2 + (y - lastY) ^ 2)
		if moved < TELEPORT then
			if UnitOnTaxi and UnitOnTaxi("player") then
				flown = flown + moved
			else
				walked = walked + moved
			end
			travelled = travelled + moved
		end
	end
	lastX, lastY, lastContinent = x, y, continent

	-- Every few minutes, so a client killed rather than logged out loses at
	-- most that much.
	if now % 300 < STEP then
		flushDistance()
	end
end

-- Sessions --------------------------------------------------------------------

--- Give the zone just left the seconds it actually had.
---
--- Not derived from the route at read time, though it could be: the route is
--- capped at `MAX_EVENTS` and a long evening drops its tail, which would
--- silently make the last hours of every raid night belong to nowhere.
---
--- `into` is where the character is going, and is only used to decide whether
--- anything is being left at all.
function creditZone(into)
	if lastZone and lastZone ~= "" and lastZone ~= into and zoneSince > 0 then
		-- Keyed by the map and labelled with the name. There are two Nagrands
		-- and two Shadowmoon Valleys, and a tally keyed by the string would
		-- have added an afternoon in Outland to an afternoon in Draenor and
		-- called it one place. The label is what a person reads; the key is
		-- what the zone corpus and the chronicle both join on.
		--
		-- Falls back to the name where the client gives no map id, which is
		-- worse than nothing only if two such zones share a name.
		tally("zone", lastMap or lastZone, time() - zoneSince, lastZone)
	end
	zoneSince = time()
end

local function whereAmI()
	local zone = GetZoneText()
	if not zone or zone == "" then
		zone = GetRealZoneText() or ""
	end
	local subzone = GetSubZoneText()
	if subzone == "" then
		subzone = nil
	end
	-- The map id as well as the name, because the name is not unique. There
	-- are two Nagrands and two Shadowmoon Valleys, and anything joining a
	-- session to a place — the zone tally, the lore corpus — joins on a string
	-- that means two different continents unless this is carried.
	local map = C_Map and C_Map.GetBestMapForUnit and C_Map.GetBestMapForUnit("player")
	return zone, subzone, map
end

--- Keep whatever number is real.
---
--- At logout `GetMoney` answers 0 and `GetAverageItemLevel` answers 0, so a
--- plain assignment on the way out replaces a session's real figures with
--- zeroes — and a journal entry then reports that the evening cost the
--- character every copper they had. The collector has the same trap and the
--- same guard; this is the second time it has been worth writing down.
--- Guarded on `current` as well as on the value: `PLAYER_MONEY` can arrive
--- between a logout closing the session and the client shutting down, and a
--- Lua error in an addon is a red box the player has to dismiss.
local function keep(field, value)
	if current and value and value > 0 then
		current[field] = value
	end
end

--- Where every faction stood, keyed by id.
---
--- Snapshotted at the start of a session and compared at the end. Reputation
--- has no event worth listening to — `CHAT_MSG_COMBAT_FACTION_CHANGE` is a
--- localised sentence, and parsing one is the thing this addon does not do —
--- and the interesting fact is not the points anyway. It is the *threshold*:
--- going from Honored to Revered with a faction is a milestone somebody
--- remembers, and 340 more reputation is not.
local standings = {}

local function snapshotStandings()
	wipe(standings)
	if not C_Reputation or not C_Reputation.GetNumFactions then
		return
	end
	for index = 1, C_Reputation.GetNumFactions() do
		local faction = C_Reputation.GetFactionDataByIndex(index)
		if faction and not faction.isHeader and faction.factionID then
			standings[faction.factionID] = { faction.name, faction.reaction }
		end
	end
end

--- Which factions moved up a rank while this session ran.
---
--- Written as a field on the session rather than as events, and so is the kill
--- count. Both are summaries of the whole evening rather than things that
--- happened at a moment in it — and, practically, both are produced at logout,
--- when the event list may already be at `MAX_EVENTS` and silently dropping
--- everything handed to it.
local function risenWith()
	local risen = {}
	if not C_Reputation or not C_Reputation.GetNumFactions then
		return risen
	end
	for index = 1, C_Reputation.GetNumFactions() do
		local faction = C_Reputation.GetFactionDataByIndex(index)
		if faction and not faction.isHeader and faction.factionID then
			local before = standings[faction.factionID]
			if before and faction.reaction and before[2] and faction.reaction > before[2] then
				risen[#risen + 1] = { faction.name, faction.reaction }
			end
		end
	end
	return risen
end

--- Close the session that is running and file it.
---
--- A session with nothing in it is dropped rather than saved. Logging in to
--- check the mail is not an evening worth writing about, and a list of empty
--- entries is what stops somebody looking at the list at all.
local function closeSession()
	if not current then
		return
	end

	-- How much was killed, as one number rather than as thousands of events.
	-- "About two hundred things in Azj-Kahet" is a fact about an evening that
	-- nothing else records; two hundred rows saying so is a file nobody loads.
	current.kills = kills
	current.risen = risenWith()
	-- Three numbers that only mean anything at the end. The worst hit and the
	-- lowest the health bar got are what a journal entry is made of; the
	-- longest fight is the difference between an evening of trash and one boss
	-- that took eleven minutes.
	current.worstHit = worstHit
	current.worstHitBy = worstHitBy
	current.longestFight = longestFight
	current.lowestHealth = lowestHealth
	-- Diagnostic; see `cleuSeen`. Read straight out of the file rather than
	-- through the desktop application, which does not know this key and
	-- ignores it.
	current.cleu = { cleuSeen, cleuKills, cleuHits, cleuMine, playerGUID or "no-guid" }
	current.travelled = math.floor(travelled)

	-- The zone being sat in when the client closed had no zone change to end
	-- it, and a logout in a raid is most of a raid night.
	creditZone(nil)
	flushDistance()

	current.endedAt = time()
	keep("endLevel", UnitLevel("player"))
	keep("endMoney", GetMoney())
	keep("endItemLevel", select(2, GetAverageItemLevel()))

	if #current.events > 0 then
		local store = db()
		store.sessions[#store.sessions + 1] = current
		while #store.sessions > MAX_SESSIONS do
			table.remove(store.sessions, 1)
		end
	end

	current = nil
end

local function openSession()
	closeSession()

	local name = UnitName("player")
	local realm = GetRealmName()
	if not name or not realm then
		return
	end

	local _, class = UnitClass("player")
	local _, race = UnitRace("player")
	local zone, subzone = whereAmI()
	local money = GetMoney()

	current = {
		startedAt = time(),
		name = name,
		realm = realm,
		class = class,
		race = race,
		faction = UnitFactionGroup("player"),
		startLevel = UnitLevel("player"),
		startMoney = money,
		startItemLevel = select(2, GetAverageItemLevel()),
		-- Seeded with the opening figures so that a session ending in a logout,
		-- where every one of these reads as zero, still closes with the truth.
		endLevel = UnitLevel("player"),
		endMoney = money,
		endItemLevel = select(2, GetAverageItemLevel()),
		events = {},
	}

	lastZone, lastSubzone, lastMap = zone, subzone, select(3, whereAmI())
	-- `GetZoneText` answers an empty string until the world has finished
	-- loading, and a login lands here before it has. A real session opened
	-- with `{ 0, "zone", "", "", 2537 }` — a map id and no place. Left
	-- unrecorded, because `lastZone` is now "" and the first `ZONE_CHANGED`
	-- after the load will note it properly a second later.
	if zone and zone ~= "" then
		note("zone", zone, subzone, select(3, whereAmI()))
	end

	-- Every carry-over cleared in one place. A campaign already named, a kill
	-- count, a rank already reached: any of them left standing from the last
	-- evening is a fact reported about the wrong one.
	wipe(seenMail)
	wipe(seenCampaign)
	wipe(seenSaid)
	saidCount = 0
	wipe(seenGossip)
	gossipCount = 0
	wipe(skills)
	wipe(equipped)
	lastInstance = nil
	killedBy = nil
	kills = 0
	playerGUID = UnitGUID("player")
	context = nil
	repairCost = 0
	listedAt = 0
	questPaid = false
	purse = money
	wipe(mailMoney)
	wipe(seenParty)
	zoneSince = time()
	walked, flown, travelled = 0, 0, 0
	lastX, lastY, lastContinent = nil, nil, nil
	fightSince = 0
	longestFight = 0
	worstHit, worstHitBy = 0, nil
	lowestHealth = 100
	cleuSeen, cleuKills, cleuHits, cleuMine = 0, 0, 0, 0

	snapshotStandings()

	-- Baselines for the two things measured as deltas. Without these the first
	-- skill-up and the first gear swap of an evening both read as new when
	-- neither is.
	local first, second, _, fishing, cooking = GetProfessions()
	for _, slot in ipairs({ first, second, fishing, cooking }) do
		if slot then
			local profession, _, rank = GetProfessionInfo(slot)
			if profession and rank then
				skills[profession] = rank
			end
		end
	end
	for slot = 1, 17 do
		local link = GetInventoryItemLink("player", slot)
		if link then
			equipped[slot] = select(4, C_Item.GetItemInfo(link))
		end
	end

	if noteInstance then
		noteInstance()
	end
end

--- Where the character went, in order.
---
--- Deduplicated against the previous entry rather than against every entry
--- seen: Orgrimmar → Durotar → Orgrimmar is a route, and collapsing it to a set
--- loses the shape of the evening.
local function noteZone()
	local zone, subzone = whereAmI()
	if zone == lastZone and subzone == lastSubzone then
		return
	end
	-- Credit the zone being left with the time actually spent in it. Only on
	-- the *zone* changing: walking from one subzone of Nagrand to another has
	-- not left Nagrand, and closing the interval there would turn one
	-- afternoon into forty entries for the same place.
	creditZone(zone)
	local map = select(3, whereAmI())
	lastZone, lastSubzone, lastMap = zone, subzone, map
	if zone and zone ~= "" then
		note("zone", zone, subzone, map)
	end
	-- Zoning is also how you get into and out of an instance, and the instance
	-- is what tells a Mythic+ run from walking through the front door.
	if noteInstance then
		noteInstance()
	end
end

-- Quests ----------------------------------------------------------------------

--- Trim a wall of quest text down to something worth carrying.
---
--- Cut at a sentence end where there is one and at a word boundary otherwise.
--- Cutting at a fixed byte count would land inside a multi-byte character in
--- every quest with an accented name in it, and half a character is worse than
--- a slightly shorter sentence.
local function gist(text)
	if not text or text == "" then
		return nil
	end
	text = text:gsub("%s+", " "):gsub("^ ", ""):gsub(" $", "")
	if #text <= QUEST_TEXT then
		return text
	end

	local cut = text:sub(1, QUEST_TEXT)
	local stop = cut:match("^.*()%. ")
	if stop and stop > QUEST_TEXT / 2 then
		return cut:sub(1, stop)
	end
	local space = cut:match("^.*() ")
	if space then
		return cut:sub(1, space - 1) .. "…"
	end
	return cut .. "…"
end

-- Wiring ----------------------------------------------------------------------

local handlers = {}

handlers.PLAYER_ENTERING_WORLD = function()
	-- A zone-in mid-session — a dungeon portal, a boat — is not a new evening.
	-- Only a login or a reload is, and both of those leave `current` nil,
	-- because a reload tears the Lua state down after `PLAYER_LOGOUT` has
	-- already filed the session that was running.
	if current then
		noteZone()
	else
		openSession()
	end
end

handlers.PLAYER_LOGOUT = closeSession

handlers.ZONE_CHANGED = noteZone
handlers.ZONE_CHANGED_INDOORS = noteZone
handlers.ZONE_CHANGED_NEW_AREA = noteZone

--- Accepting a quest: the premise, which the turn-in text assumes you remember.
--- Who is standing in front of you while a quest frame is open.
---
--- The `npc` unit token is only valid while the frame is up, which is the same
--- window the quest text is readable in and the reason both are captured here
--- rather than at `QUEST_TURNED_IN`.
---
--- Two identifiers, and they answer different questions. The **name** is what
--- connects a character across expansions — Khadgar in Outland, Khadgar in
--- Draenor and Khadgar in the Broken Isles are three different creatures in
--- Blizzard's data and one person in anybody's memory. The **creature id** is
--- what tells two NPCs who happen to share a name apart within one version.
--- Recording only the id would lose the thread this is for; recording only the
--- name would merge every guard called "Stormwind Guard" into one acquaintance.
local function whoIsTalking()
	if not UnitExists or not UnitExists("npc") then
		return nil, nil
	end
	local name = UnitName("npc")
	-- `Creature-0-serverID-instanceID-zoneUID-creatureID-spawnUID`. The sixth
	-- field is the one that is stable across every copy of that NPC.
	local guid = UnitGUID("npc")
	local creature = guid and tonumber(select(6, strsplit("-", guid)))
	return name, creature
end

--- Remember the quest giver alongside the quest.
---
--- Its own row rather than a field on the quest, because a quest moment already
--- fills all five of its positions with the id, the title and the story text —
--- and because "who did I talk to" is a question worth asking of an evening
--- even for the quests that were abandoned.
local function noteGiver(questID)
	local name, creature = whoIsTalking()
	if name and name ~= "" then
		note("giver", name, questID or "", creature or "")
	end
end

handlers.QUEST_DETAIL = function()
	local title = GetTitleText()
	if title and title ~= "" then
		note("accepted", title, gist(GetQuestText()))
		noteGiver(GetQuestID and GetQuestID() or nil)
	end
end

--- The reward frame is up, which is the one moment the story text is readable.
--- `QUEST_TURNED_IN` follows with the id, by which time the frame is gone.
handlers.QUEST_COMPLETE = function()
	local name, creature = whoIsTalking()
	pending = {
		title = GetTitleText(),
		text = gist(GetRewardText()) or gist(GetObjectiveText()),
		giver = name,
		creature = creature,
	}
end

--- Which story a quest belongs to, recorded once per campaign per session.
---
--- The most valuable thing in this file after the quest text itself. A dozen
--- turn-ins in an evening is a dozen titles and no shape; `GetCampaignID` and
--- `GetCampaignInfo` say that eight of them were chapters of *The Severed
--- Threads* and hand over Blizzard's own paragraph describing it. That turns a
--- list into an arc, which is the difference between a log and a journal.
---
--- Nothing in the web API knows campaigns exist.
local function noteCampaign(questID)
	if not C_CampaignInfo or not questID then
		return
	end
	local campaignID = C_CampaignInfo.GetCampaignID(questID)
	if not campaignID or campaignID == 0 or seenCampaign[campaignID] then
		return
	end
	seenCampaign[campaignID] = true

	local info = C_CampaignInfo.GetCampaignInfo(campaignID)
	if info and info.name and info.name ~= "" then
		note("campaign", info.name, gist(info.description))
	end
end

handlers.QUEST_TURNED_IN = function(questID, xp, money)
	local title = C_QuestLog.GetTitleForQuestID(questID)
	if (not title or title == "") and pending then
		title = pending.title
	end
	noteCampaign(questID)
	note("quest", questID, title, pending and pending.text or nil)
	-- Captured at `QUEST_COMPLETE`, because the frame — and with it the `npc`
	-- token — is gone by the time the id arrives here.
	if pending and pending.giver then
		note("giver", pending.giver, questID, pending.creature or "")
		tally("questgiver", pending.giver, 1, pending.giver)
	end

	if money and money > 0 then
		-- Kept beside the quest rather than folded into the session's gold
		-- delta, so "the escort paid better than the whole afternoon of
		-- herbalism" stays a sayable thing. The flag stops the ledger counting
		-- the same gold a second time as something found on the ground.
		questPaid = true
		note("questpay", questID, money, xp or 0)
	end
	pending = nil
end

handlers.PLAYER_LEVEL_UP = function(level)
	note("level", level, (whereAmI()))
	capture("level", tostring(level))
end

--- Who hit you last, from the combat log.
---
--- `PLAYER_DEAD` says a character died and never says what to. "Died to a
--- Gorian Warlock at Halaa" is a story beat; "died in Nagrand" is a
--- coordinate. This is the whole reason the combat log is read at all.
handlers.PLAYER_DEAD = function()
	local zone, subzone = whereAmI()
	note("death", zone, subzone, killedBy)
	-- Which of them keeps doing it is a fact about months rather than about
	-- tonight, and is funnier and more useful than the count of deaths.
	if killedBy then
		tally("killer", killedBy, 1, killedBy)
	end
	killedBy = nil
	-- A death ends the fight, and a corpse run is not an eleven-minute boss.
	fightSince = 0
end

handlers.PLAYER_MONEY = function()
	keep("endMoney", GetMoney())
	noteCoin()
end

-- Every frame that takes or gives money. The event is only ever "something
-- opened" or "something closed"; the ledger above does the rest.
handlers.MERCHANT_SHOW = function()
	enter("vendor")
end
handlers.MERCHANT_CLOSED = leave

handlers.AUCTION_HOUSE_SHOW = function()
	enter("auction")
end
handlers.AUCTION_HOUSE_CLOSED = leave

handlers.AUCTION_HOUSE_AUCTION_CREATED = function()
	listedAt = time()
end

handlers.MAIL_SHOW = function()
	enter("mail")
end
handlers.MAIL_CLOSED = leave

handlers.TRADE_SHOW = function()
	enter("trade")
end
handlers.TRADE_CLOSED = leave

handlers.TAXIMAP_OPENED = function()
	enter("taxi")
end

--- Taking a flight, as opposed to opening the map and thinking better of it.
---
--- The map closing is not the flight; `UnitOnTaxi` a moment later is. Counting
--- the frame instead would credit somebody with a hundred flights for standing
--- at a flight master reading the map.
handlers.TAXIMAP_CLOSED = function()
	leave()
	local from = (whereAmI())
	C_Timer.After(1, function()
		if UnitOnTaxi and UnitOnTaxi("player") and from and from ~= "" then
			note("flight", from)
			tally("flight", from, 1, from)
		end
	end)
end

handlers.TRAINER_SHOW = function()
	enter("trainer")
end
handlers.TRAINER_CLOSED = leave

handlers.TRANSMOGRIFY_OPEN = function()
	enter("transmog")
end
handlers.TRANSMOGRIFY_CLOSE = leave

handlers.BARBER_SHOP_OPEN = function()
	enter("barber")
end
handlers.BARBER_SHOP_CLOSE = leave

handlers.GUILDBANKFRAME_OPENED = function()
	enter("guildbank")
end
handlers.GUILDBANKFRAME_CLOSED = leave

handlers.BOSS_KILL = function(_, name)
	note("boss", name)
end

--- The combat log, read for two things and ignored for everything else.
---
--- CLEU is the firehose in this game: a raid night is tens of thousands of
--- events and reading it properly is what WarcraftLogs is *for*. Nothing here
--- tries to be that. Two questions are asked of it, both of which no other
--- event answers, and every other subevent is dropped in the first comparison:
---
--- * **What killed you.** The last thing to damage the player before
---   `PLAYER_DEAD`.
--- * **What you killed that had a name worth writing down.** `PARTY_KILL`
---   fires for anything the party finishes, so it also gives an honest count
---   of how much was cleared — "about two hundred things in Azj-Kahet" is a
---   fact about an evening that no other source has.
---
--- The named ones are separated from the tally by classification, which is
--- only readable while the thing is targeted. Checking the target at the
--- moment of the kill catches the rare you were fighting and misses rares
--- somebody else in the party tagged, which is the right way round.
handlers.COMBAT_LOG_EVENT_UNFILTERED = function()
	if not current then
		return
	end

	local _, subevent, _, sourceGUID, sourceName, _, _, destGUID = CombatLogGetCurrentEventInfo()
	cleuSeen = cleuSeen + 1
	if destGUID == playerGUID then
		cleuMine = cleuMine + 1
	end

	if subevent == "PARTY_KILL" then
		cleuKills = cleuKills + 1
		kills = kills + 1
		-- `UnitClassification` needs a unit token, and the only one that can
		-- describe the thing that just died is the current target.
		if destGUID and UnitGUID("target") == destGUID then
			local rank = UnitClassification("target")
			if rank == "rare" or rank == "rareelite" or rank == "worldboss" then
				local name = UnitName("target")
				note("rare", name, rank)
				-- Counted for life as well as for the evening. A world rare
				-- raises no ENCOUNTER_END, so this is the only record of how
				-- many times somebody has gone back for the thing it drops.
				tally("rare", name, 1, name)
				capture("rare", name)
			end
		end
		return
	end

	-- Anything that damaged the player. Held rather than noted, because most
	-- of them are followed by the player not dying.
	if
		destGUID == playerGUID
		and sourceName
		and sourceName ~= ""
		and sourceGUID ~= destGUID
		and subevent:find("_DAMAGE", 1, true)
	then
		cleuHits = cleuHits + 1
		killedBy = sourceName

		-- The two numbers a person actually retells. Eleven arguments are
		-- common to every subevent and the payload follows, so the amount is
		-- read by position from the front — a swing puts it first, a spell
		-- after the three that name the spell, and the environment after the
		-- one that says what the environment was. Counting from the *end*
		-- would be shorter and wrong: a trailing nil shortens the list.
		local amount
		if subevent:find("^SWING") then
			amount = select(12, CombatLogGetCurrentEventInfo())
		elseif subevent:find("^ENVIRONMENTAL") then
			amount = select(13, CombatLogGetCurrentEventInfo())
		else
			amount = select(15, CombatLogGetCurrentEventInfo())
		end
		amount = tonumber(amount)
		if amount and amount > worstHit then
			worstHit, worstHitBy = amount, sourceName
		end

		local health = UnitHealth("player") or 0
		local maximum = UnitHealthMax("player") or 0
		if maximum > 0 and health > 0 then
			local percent = math.floor(health / maximum * 100)
			if percent < lowestHealth then
				lowestHealth = percent
			end
		end
	end
end

--- How long the longest fight of the evening was.
---
--- Combat start and end, which the game does raise events for. A boss that
--- took eleven minutes and an evening of six-second pulls are the same "1
--- boss" without this.
handlers.PLAYER_REGEN_DISABLED = function()
	fightSince = time()
end

handlers.PLAYER_REGEN_ENABLED = function()
	if fightSince > 0 then
		local length = time() - fightSince
		if length > longestFight then
			longestFight = length
		end
		fightSince = 0
	end
end

--- A wipe is as much of a story as a kill, and more of one on the tenth
--- attempt, so both are recorded and the outcome is a field.
handlers.ENCOUNTER_END = function(_, name, difficulty, _, success)
	note("encounter", name, success == 1 and 1 or 0, difficulty)
	if not name or name == "" then
		return
	end
	-- Attempts and defeats separately rather than a ratio, because the ratio
	-- can be computed from the two and neither can be recovered from it. The
	-- interesting number is "eleven pulls", and Blizzard forgets a pull the
	-- moment it ends.
	tally("attempt", name, 1, name)
	if success == 1 then
		tally("victory", name, 1, name)
	end
end

handlers.ACHIEVEMENT_EARNED = function(achievementID)
	local _, name = GetAchievementInfo(achievementID)
	note("achievement", achievementID, name)
	capture("achievement", name)
end

handlers.NEW_MOUNT_ADDED = function(mountID)
	local name = C_MountJournal.GetMountInfoByID(mountID)
	note("gained", "mount", name)
	capture("mount", name)
end

handlers.NEW_TOY_ADDED = function(itemID)
	note("gained", "toy", (select(2, C_ToyBox.GetToyInfo(itemID))))
end

handlers.NEW_PET_ADDED = function(petGUID)
	local speciesID = C_PetJournal.GetPetInfoByPetID(petGUID)
	if speciesID then
		note("gained", "pet", (C_PetJournal.GetPetInfoBySpeciesID(speciesID)))
	end
end

handlers.CHAT_MSG_LOOT = function(message)
	-- Every loot line carries an item link, and the id inside it is the only
	-- part worth reading. Matching the sentence around it would mean matching a
	-- localised `LOOT_ITEM_SELF` format string.
	local itemID = message and tonumber(message:match("|Hitem:(%d+)"))
	if not itemID then
		return
	end
	local name, _, quality = C_Item.GetItemInfo(itemID)
	-- An item the client has not cached answers nil. Skipped rather than
	-- guessed: an id with no name is not something to write a sentence about.
	if name and quality and quality >= LOOT_QUALITY then
		note("loot", itemID, name, quality)
	end
end

--- Whether a name is another character on this account.
---
--- The collector records every character it has run on, and mail from an alt
--- carries either `Name` or `Name-Realm` depending on whether the realm is the
--- same. Both are checked against the same list.
local function isMine(sender)
	local roster = ArmoryCollectorDB and ArmoryCollectorDB.roster
	if not roster or not sender then
		return false
	end
	if roster[sender] then
		return true
	end
	local realm = GetRealmName()
	return realm ~= nil and roster[sender .. "-" .. realm] == true
end

handlers.MAIL_INBOX_UPDATE = function()
	for index = 1, (GetInboxNumItems() or 0) do
		-- `packageIcon, stationeryIcon, sender, subject, money, CODAmount,
		--  daysLeft, itemCount, wasRead, ...`
		local _, _, sender, subject, money = GetInboxHeaderInfo(index)
		if sender and subject then
			local key = sender .. "|" .. subject .. "|" .. (money or 0)
			if not seenMail[key] then
				seenMail[key] = true

				-- An auction that came back. The auction house sends it with
				-- no money attached, which is exactly what distinguishes it
				-- from a sale — and it is the only evidence anywhere that
				-- something did *not* sell. Blizzard records no failure any
				-- more than it records a sale.
				if sender:find("Auction") and (not money or money == 0) then
					note("expired", subject)
				end

				if money and money > 0 then
					-- Three different facts wearing one event. The auction
					-- house paying is income; an alt sending gold over is a
					-- transfer and is not income at all; anybody else is a
					-- gift. Recorded by amount, because the sender is
					-- readable now and gone by the time the money lands.
					if sender:find("Auction") then
						mailMoney[money] = "sale"
						-- Only the auction house's subject line names an
						-- item, and only its money is a sale. Everything else
						-- in a mailbox is somebody else's business and none
						-- of it belongs in a file another program reads.
						note("sale", subject, money)
					elseif isMine(sender) then
						mailMoney[money] = "transfer"
					else
						mailMoney[money] = "gift"
					end
				end
			end
		end
	end
end

--- Which instance the character is standing in, and at what difficulty.
---
--- Without this a Mythic+ run, a heroic raid night and walking through the
--- front door of the same building are all one zone name. `instanceType` and
--- `difficultyName` are the difference between "Halls of Atonement" and "Halls
--- of Atonement, Mythic Keystone, five of us".
---
--- Assigned rather than declared: the local is forward-declared at the top so
--- `openSession` can reach it.
function noteInstance()
	local name, kind, _, difficulty, _, _, _, _, size = GetInstanceInfo()
	if not name or kind == "none" or kind == nil then
		lastInstance = nil
		return
	end
	local key = name .. "|" .. (difficulty or "")
	if key == lastInstance then
		return
	end
	lastInstance = key
	note("instance", name, kind .. (difficulty and difficulty ~= "" and (", " .. difficulty) or ""), size or 0)
end

--- A keystone finished, timed or not.
---
--- `time` is milliseconds, `onTime` is whether the timer held, and
--- `keystoneUpgradeLevels` is how many the key went up by — which is the
--- number the group actually cared about.
handlers.CHALLENGE_MODE_COMPLETED = function()
	if not C_ChallengeMode then
		return
	end
	local _, level, elapsed, onTime, upgrades = C_ChallengeMode.GetCompletionInfo()
	if not level then
		return
	end
	local name = GetInstanceInfo()
	note(
		"keystone",
		(name or "") .. "|" .. level .. "|" .. (onTime and 1 or 0) .. "|" .. (upgrades or 0),
		math.floor((elapsed or 0) / 1000)
	)
	capture("keystone", (name or "") .. " +" .. level)
end

--- A scenario or delve finished.
---
--- A delve is a scenario as far as `GetInstanceInfo` is concerned, and the
--- tier is the whole difference between one and another — a tier 11 delve and
--- a tier 2 delve are the same three words otherwise. `GetActiveDelveTier`
--- answers only while one is active, which is why it is read here and not at
--- logout.
---
--- Brann's own level needs nothing: it is a Warband reputation, so it already
--- arrives through the reputation path like every other faction.
handlers.SCENARIO_COMPLETED = function()
	local name, kind = GetInstanceInfo()
	if not name or kind ~= "scenario" then
		return
	end

	local tier = C_DelvesUI and C_DelvesUI.GetActiveDelveTier and C_DelvesUI.GetActiveDelveTier()
	if tier and tier.tier then
		local said = "Tier " .. tier.tier
		note("scenario", name, said)
		tally("delve", said, 1, said)
	else
		note("scenario", name)
	end
end

--- A profession got better at something.
---
--- `CHAT_MSG_SKILL` is the localised "Your skill in Alchemy has increased to
--- 84" line. The number is not parsed out of it — that would mean matching a
--- localised format string — so the professions are re-read instead and the
--- ones that moved are noted.
handlers.CHAT_MSG_SKILL = function()
	local first, second, _, fishing, cooking = GetProfessions()
	for _, slot in ipairs({ first, second, fishing, cooking }) do
		if slot then
			local name, _, rank = GetProfessionInfo(slot)
			if name and rank and skills[name] and rank > skills[name] then
				note("skill", name, rank)
			end
			if name and rank then
				skills[name] = rank
			end
		end
	end
end

--- Something better got equipped.
---
--- Only when the item level actually went up. Swapping a trinket for a fight
--- and swapping it back is not an upgrade, and a journal that reports both as
--- news is one that buries the evening's real one.
handlers.PLAYER_EQUIPMENT_CHANGED = function(slot)
	if not slot then
		return
	end
	local link = GetInventoryItemLink("player", slot)
	if not link then
		equipped[slot] = nil
		return
	end
	local name, _, _, level = C_Item.GetItemInfo(link)
	if not name or not level then
		return
	end
	local before = equipped[slot]
	equipped[slot] = level
	if before and level > before then
		note("equipped", name, level, level - before)
	end
end

--- Something was made.
---
--- `UNIT_SPELLCAST_SUCCEEDED` fires for every spell the player finishes, and a
--- craft is a spell. `GetRecipeInfo` is what separates the two: it answers for
--- a recipe and nothing else, so a cast that it names is a craft and a cast it
--- does not is a fireball.
---
--- Counted twice on purpose. The session gets one event per craft, because
--- "made forty flasks" is a fact about an evening. The account file gets a
--- running total per recipe, because "has ever made four hundred flasks" is a
--- fact about a character — and the game keeps neither. Blizzard's statistics
--- have coarse counters for some professions; there is nothing anywhere that
--- counts a *particular* recipe.
handlers.UNIT_SPELLCAST_SUCCEEDED = function(unit, _, spellID)
	if unit ~= "player" or not spellID or not C_TradeSkillUI then
		return
	end
	local recipe = C_TradeSkillUI.GetRecipeInfo and C_TradeSkillUI.GetRecipeInfo(spellID)
	if not recipe or not recipe.name or recipe.name == "" then
		return
	end

	-- `GetRecipeInfo` answers for spells that are not recipes at all. A real
	-- session recorded a login visual (`LOGINEFFECT`) and a hidden
	-- quest-tracking spell (`Flag Tracking Quest [DNT]`) as crafts, because
	-- both came back with a name. A name is not enough.
	--
	-- The book this character's own profession window gave us is the exact
	-- test where we have it; where we do not, the game calling it learned and
	-- filing it under a category is the next best thing. A login effect is
	-- neither.
	local me = whoami()
	local book = me
		and ArmoryCollectorDB
		and ArmoryCollectorDB.recipes
		and ArmoryCollectorDB.recipes[me]
	local known = book and book[spellID] ~= nil
	if not known and not (recipe.learned and recipe.categoryID) then
		return
	end

	note("craft", spellID, recipe.name)
	tally("recipe", spellID, 1, recipe.name)
end

--- A recipe learned.
---
--- Worth an evening's card in its own right — "learned to make Flasks of
--- Alchemical Chaos" is a thing that happened, and a profession rank going up
--- by one is not the same fact. The collector beside this records what the
--- recipe *takes*; this records that it arrived.
handlers.NEW_RECIPE_LEARNED = function(recipeID)
	if not recipeID or not C_TradeSkillUI or not C_TradeSkillUI.GetRecipeInfo then
		return
	end
	local info = C_TradeSkillUI.GetRecipeInfo(recipeID)
	if info and info.name and info.name ~= "" then
		note("recipe", info.name)
	end
end

--- What the world said while you were in it.
---
--- The scripted lines: an NPC talking to another NPC, a boss yelling mid-pull,
--- an escort narrating itself, the emote a rare does before it charges. That
--- is written content the player read and no endpoint has any of it — the same
--- argument as the quest text, applied to everything that happens between the
--- quests.
---
--- **Only NPCs.** `CHAT_MSG_MONSTER_*` and `CHAT_MSG_RAID_BOSS_*` are the
--- game's own writing. Player chat is not, and the events that carry it are
--- deliberately not registered: what somebody said in party is their business
--- and none of it belongs in a file another program reads. The same rule the
--- mailbox scan already follows.
local function overheard(text, who, emote)
	if not current or not text or text == "" or saidCount >= MAX_SAID then
		return
	end
	who = who or ""
	-- An emote's text carries a `%s` where the name goes, which is the game's
	-- own formatting and reads as a bug if it is written through.
	if emote and who ~= "" then
		text = text:gsub("%%s", who)
	end
	if #text > SAID_TEXT then
		text = text:sub(1, SAID_TEXT) .. "…"
	end

	local key = who .. "|" .. text
	if seenSaid[key] then
		return
	end
	seenSaid[key] = true
	saidCount = saidCount + 1
	note("said", who, text)
end

--- What an NPC says when you click them.
---
--- Different from `overheard` above and kept apart from it on purpose: this is
--- something the player chose to read, not something they happened to be
--- standing next to. The distinction is what lets the journal's instructions
--- say "much of this is functional, use it only where it carries the evening"
--- — a rule that would be wrong applied to a boss mid-fight.
---
--- Deliberately unfiltered beyond deduplication. A length threshold would drop
--- a short line that mattered and keep a long one that did not, and the reader
--- best placed to tell the difference is the one writing the entry.
handlers.GOSSIP_SHOW = function()
	if not current or gossipCount >= MAX_GOSSIP or not C_GossipInfo then
		return
	end
	local text = C_GossipInfo.GetText and C_GossipInfo.GetText()
	if not text or text == "" then
		return
	end
	if #text > SAID_TEXT then
		text = text:sub(1, SAID_TEXT) .. "…"
	end
	if seenGossip[text] then
		return
	end
	seenGossip[text] = true
	gossipCount = gossipCount + 1
	-- The gossip unit is `npc`; a click that left the target set answers when
	-- it does not.
	note("gossip", UnitName("npc") or UnitName("target") or "", text)
end

handlers.CHAT_MSG_MONSTER_SAY = function(text, who)
	overheard(text, who)
end
handlers.CHAT_MSG_MONSTER_YELL = function(text, who)
	overheard(text, who)
end
handlers.CHAT_MSG_MONSTER_WHISPER = function(text, who)
	overheard(text, who)
end
handlers.CHAT_MSG_MONSTER_EMOTE = function(text, who)
	overheard(text, who, true)
end
handlers.CHAT_MSG_RAID_BOSS_EMOTE = function(text, who)
	overheard(text, who, true)
end
handlers.CHAT_MSG_RAID_BOSS_WHISPER = function(text, who)
	overheard(text, who)
end

--- An NPC travelling with you. Escort followers, bodyguards, the dragon on the
--- way to the thing. Its own event, and missing it loses exactly the dialogue
--- that belongs to a quest rather than to a zone.
handlers.CHAT_MSG_MONSTER_PARTY = function(text, who)
	overheard(text, who)
end

--- The talking-head bar, which is where a great deal of modern quest dialogue
--- actually is.
---
--- Since Legion, a lot of what used to be an NPC saying something in the chat
--- frame is delivered through the cinematic bar at the top of the screen
--- instead. It raises no chat event at all, so an addon reading only
--- `CHAT_MSG_MONSTER_*` silently misses it — and it is the most deliberately
--- written dialogue in the game, because it is the part Blizzard paid to have
--- voiced.
---
--- `GetCurrentLineInfo` answers
--- `displayInfo, cameraID, vo, duration, lineNumber, numLines, name, text`.
handlers.TALKINGHEAD_REQUESTED = function()
	if not C_TalkingHead or not C_TalkingHead.GetCurrentLineInfo then
		return
	end
	local _, _, _, _, _, _, name, text = C_TalkingHead.GetCurrentLineInfo()
	overheard(text, name)
end

--- A cutscene played.
---
--- Two different things wear the name, and only one of them can be identified
--- afterwards:
---
--- * **A pre-rendered movie** raises `PLAY_MOVIE` with a `MovieID`. That is a
---   stable number and names the cinematic exactly, which is the whole point of
---   recording it — "the Legion end cinematic" is a fact somebody can look up.
--- * **An in-engine cutscene** raises `CINEMATIC_START` with no id at all.
---   Nothing identifies which one it was, so what is recorded is that one
---   happened, where, and when — and the quest turned in thirty seconds later
---   is what names it in practice.
---
--- `IsInCinematicScene` is what separates a real cutscene from the camera pan
--- the game does at a flight path or a race intro. Without it every taxi ride
--- files a cutscene.
---
--- Subtitles are not readable. They live in the client's own data files and no
--- API exposes them. What *is* readable is the dialogue: an in-engine cutscene
--- usually speaks through `CHAT_MSG_MONSTER_SAY`, so the words are already
--- being captured by `overheard` above — which is as close to a transcript as
--- this can get.
handlers.CINEMATIC_START = function()
	if IsInCinematicScene and IsInCinematicScene() then
		note("cutscene", (whereAmI()))
	end
end

handlers.PLAY_MOVIE = function(movieID)
	note("cutscene", (whereAmI()), movieID)
end

--- An appearance the account had never seen.
handlers.TRANSMOG_COLLECTION_SOURCE_ADDED = function(sourceID)
	if not C_TransmogCollection or not sourceID then
		return
	end
	local info = C_TransmogCollection.GetSourceInfo(sourceID)
	if info and info.name and info.name ~= "" then
		note("appearance", info.name)
	end
end

--- Who was there.
---
--- Names only, and parties only. A raid of twenty strangers is a list rather
--- than a memory, and the group finder fills one every few minutes.
handlers.GROUP_ROSTER_UPDATE = function()
	local size = GetNumGroupMembers() or 0
	if size == 0 or size > 5 or IsInRaid() then
		return
	end
	for index = 1, size - 1 do
		local name = UnitName("party" .. index)
		if name and not seenParty[name] then
			seenParty[name] = true
			note("with", name)
			-- Once per evening, not once per roster update: a four-hour
			-- dungeon night fires this every time anybody zones.
			tally("companion", name, 1, name)
		end
	end
end

-- The one thing here with no event behind it. Moving raises nothing, so
-- position is sampled, and `sampleDistance` throttles itself to once a second
-- rather than running on every frame.
frame:SetScript("OnUpdate", sampleDistance)

frame:SetScript("OnEvent", function(_, event, ...)
	local handler = handlers[event]
	if handler then
		handler(...)
	end
end)

-- Guarded, because this list is long now and `RegisterEvent` throws on a name
-- the client does not know. A patch renaming one event should cost that one
-- event, not the whole addon — the file would otherwise fail to load and take
-- every other thing it records with it.
for event in pairs(handlers) do
	local ok = pcall(frame.RegisterEvent, frame, event)
	if not ok then
		handlers[event] = nil
	end
end

-- What the client refused, and to whom.
--
-- A blocked action is a dialog the player sees and a fact the addon otherwise
-- never learns: the popup names the addon and not the function, so working out
-- which call was refused means guessing. This writes the function down so the
-- next read says outright. Kept in the account file rather than the session,
-- because it is a fault report and not something that happened in the evening.
-- Guarded like the loop above, and for the same reason: an event name the
-- client does not know throws, and a fault reporter is not worth taking the
-- addon down over.
pcall(frame.RegisterEvent, frame, "ADDON_ACTION_BLOCKED")
pcall(frame.RegisterEvent, frame, "ADDON_ACTION_FORBIDDEN")
handlers["ADDON_ACTION_BLOCKED"] = function(who, what)
	if who ~= ADDON then
		return
	end
	ArmoryCollectorDB = ArmoryCollectorDB or {}
	local blocked = ArmoryCollectorDB.blocked or {}
	-- One entry per function, with a count. A blocked call in a loop would
	-- otherwise fill the file with the same line.
	blocked[what or "?"] = (blocked[what or "?"] or 0) + 1
	ArmoryCollectorDB.blocked = blocked
end
handlers["ADDON_ACTION_FORBIDDEN"] = handlers["ADDON_ACTION_BLOCKED"]

SLASH_ARMORYLOG1 = "/armorylog"
SlashCmdList["ARMORYLOG"] = function()
	if not current then
		print("|cff8ab4f8Armory|r: no session is being recorded.")
		return
	end
	print(string.format(
		"|cff8ab4f8Armory|r: %d events recorded since login. Log out or /reload to write the file.",
		#current.events
	))
end

_ = ADDON
