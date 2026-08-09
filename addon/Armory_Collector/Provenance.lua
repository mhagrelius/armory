-- Armory Provenance
--
-- Who actually earned the account's account-wide progress.
--
-- This is the reputation and currency half of the problem the whole
-- application exists for. An account-wide achievement names whoever earned it
-- first and never anybody else; an account-wide *reputation* does not even do
-- that. The War Within syncs most standings to the furthest-progressed
-- character, and Dragonflight's renown works the same way, so a character
-- created yesterday reads Exalted, reads Renown 25, and has done none of it.
--
-- Armory already refuses to count those, which is correct and not enough. A
-- person replaying the game *can* earn the equivalent of Exalted with a
-- faction the account maxed out in 2023, and there is currently no way to say
-- so — the standing was already at the ceiling before they started, so it
-- cannot move to record the work. What can be recorded is the reputation
-- itself, as it arrives, attributed to whoever was logged in at the time.
--
-- **That attribution is sound for one reason: you can only play one character
-- at a time.** One client, one character. So a standing that rises between
-- this character's login and this character's logout rose because of this
-- character. Anything that changed while they were logged out was somebody
-- else's doing and is deliberately invisible here — which is exactly right,
-- because it was.
--
-- Two snapshots per session, then, and the difference between them. No event
-- parsing, no localised strings, and nothing that breaks when a patch renames
-- a message.
--
-- Currency gets the same treatment and one extra question. A currency can also
-- *arrive* on a character without being earned by it, because the Warband can
-- move some of them — so a rise has three possible causes and this file
-- records what it can rather than picking one. See `snapshotCurrency`.

local ADDON = ...

--- Bumped when the shape of `ArmoryCollectorDB.earned` changes.
local FORMAT = 1

--- How often to fold the running session into the totals, in seconds.
---
--- Logout is the natural moment and a crash is the reason not to rely on it.
--- Five minutes costs nothing — it is two table walks — and it means a client
--- killed with the task manager loses five minutes of attribution rather than
--- an evening's.
local FOLD_EVERY = 300

local frame = CreateFrame("Frame")

--- Standings as they were at the start of this session, keyed by faction.
--- `{ points, renownLevel }`.
local repAtLogin = {}

--- Currency as it was at the start of this session, keyed by currency id.
--- `{ quantity, totalEarned }`.
local currencyAtLogin = {}

--- Whether a snapshot has been taken yet this session. Nothing folds before
--- one exists, or the first fold would attribute the character's entire
--- lifetime to a single evening.
local ready = false

local function whoami()
	local name = UnitName("player")
	local realm = GetRealmName()
	if not name or not realm then
		return nil
	end
	return name .. "-" .. realm
end

local function db()
	ArmoryCollectorDB = ArmoryCollectorDB or {}
	local store = ArmoryCollectorDB
	store.earnedFormat = FORMAT
	store.earned = store.earned or {}

	local me = whoami()
	if not me then
		return nil
	end
	store.earned[me] = store.earned[me] or { rep = {}, currency = {} }
	return store.earned[me]
end

-- Reputation ------------------------------------------------------------------

--- Every faction's standing right now.
---
--- Renown and classic reputation are one number each and they are not the same
--- number. A major faction's progress is a *level* plus a partial bar, and its
--- `currentStanding` is the bar alone — so a character who earned nine renown
--- levels and stopped mid-tenth would look, by standing alone, like they had
--- earned almost nothing. Both are recorded, and the level is the one that
--- means something to a person.
local function readStandings()
	local now = {}
	if not C_Reputation or not C_Reputation.GetNumFactions then
		return now
	end

	for index = 1, C_Reputation.GetNumFactions() do
		local faction = C_Reputation.GetFactionDataByIndex(index)
		if faction and not faction.isHeader and faction.factionID then
			local renown = 0
			if C_MajorFactions then
				local major = C_MajorFactions.GetMajorFactionData(faction.factionID)
				if major and major.renownLevel then
					renown = major.renownLevel
				end
			end

			-- Paragon accrues past Exalted and is the only progress a maxed
			-- faction can still show. Without it, a character grinding a
			-- capped faction for a mount records nothing at all.
			local paragon = 0
			if C_Reputation.GetFactionParagonInfo then
				local value, threshold = C_Reputation.GetFactionParagonInfo(faction.factionID)
				-- The value carries a prefix counting rewards already taken,
				-- which the wiki documents and which is not a reputation
				-- amount. The remainder is.
				if value and threshold and threshold > 0 then
					paragon = value % threshold
				end
			end

			now[faction.factionID] = {
				faction.currentStanding or 0,
				renown,
				paragon,
				faction.name,
				-- Whether inheritance is even possible for this one. A faction
				-- that is not account-wide cannot have been earned by anybody
				-- else, so its standing is already honest and the earned total
				-- is a cross-check rather than the only truth.
				C_Reputation.IsAccountWideReputation
						and C_Reputation.IsAccountWideReputation(faction.factionID)
						and 1
					or 0,
			}
		end
	end
	return now
end

-- Currency --------------------------------------------------------------------

--- Every currency the character can see, and the two numbers that matter.
---
--- `quantity` is what is held. `totalEarned` is what has ever been earned —
--- but only for currencies with a moving maximum, which is what
--- `useTotalEarnedForMaxQty` says; for everything else the game returns zero
--- and the field means nothing. Recording that flag alongside is what stops
--- the desktop application reading a real zero as an earned-nothing.
local function readCurrency()
	local now = {}
	if not C_CurrencyInfo or not C_CurrencyInfo.GetCurrencyListSize then
		return now
	end

	for index = 1, C_CurrencyInfo.GetCurrencyListSize() do
		local entry = C_CurrencyInfo.GetCurrencyListInfo(index)
		if entry and not entry.isHeader then
			local link = C_CurrencyInfo.GetCurrencyListLink(index)
			local id = link and tonumber(link:match("currency:(%d+)"))
			if id then
				local info = C_CurrencyInfo.GetCurrencyInfo(id)
				if info then
					now[id] = {
						info.quantity or 0,
						info.totalEarned or 0,
						info.useTotalEarnedForMaxQty and 1 or 0,
						info.isAccountWide and 1 or 0,
						info.isAccountTransferable and 1 or 0,
					}
				end
			end
		end
	end
	return now
end

-- Folding ---------------------------------------------------------------------

--- Add this session's gains to the character's lifetime totals.
---
--- Only rises count. A standing that went *down* is a faction turned hostile
--- or a currency spent, and neither un-earns the work that put it there.
---
--- Re-snapshots afterwards, so this is safe to call repeatedly: what has been
--- folded is never folded twice.
local function fold()
	if not ready then
		return
	end
	local mine = db()
	if not mine then
		return
	end

	local now = readStandings()
	for factionID, after in pairs(now) do
		local before = repAtLogin[factionID]
		if before then
			local points = math.max(0, (after[1] or 0) - (before[1] or 0))
			local renown = math.max(0, (after[2] or 0) - (before[2] or 0))
			local paragon = math.max(0, (after[3] or 0) - (before[3] or 0))
			if points > 0 or renown > 0 or paragon > 0 then
				local held = mine.rep[factionID] or { 0, 0, 0, after[4], after[5] }
				held[1] = (held[1] or 0) + points + paragon
				held[2] = (held[2] or 0) + renown
				-- Highest standing this character has personally reached with
				-- them, which is not the same as the account's and is the
				-- number a replay wants.
				held[3] = math.max(held[3] or 0, after[2] or 0)
				held[4] = after[4]
				held[5] = after[5]
				mine.rep[factionID] = held
			end
		end
	end
	repAtLogin = now

	local currency = readCurrency()
	for id, after in pairs(currency) do
		local before = currencyAtLogin[id]
		if before then
			local gained = math.max(0, (after[1] or 0) - (before[1] or 0))
			local earned = math.max(0, (after[2] or 0) - (before[2] or 0))
			if gained > 0 or earned > 0 then
				local held = mine.currency[id] or { 0, 0, after[4], after[5], after[3] }
				-- Both numbers, unreconciled on purpose. `gained` is what
				-- arrived and `earned` is what the game says was earned, and
				-- the gap between them is a transfer from another character —
				-- but only where `totalEarned` means anything at all. Deciding
				-- which is which is the desktop application's job, where the
				-- reasoning can be written down and tested.
				held[1] = (held[1] or 0) + gained
				held[2] = (held[2] or 0) + earned
				held[3] = after[4]
				held[4] = after[5]
				held[5] = after[3]
				mine.currency[id] = held
			end
		end
	end
	currencyAtLogin = currency
end

local function snapshot()
	repAtLogin = readStandings()
	currencyAtLogin = readCurrency()
	ready = true
end

-- Wiring ----------------------------------------------------------------------

frame:RegisterEvent("PLAYER_ENTERING_WORLD")
frame:RegisterEvent("PLAYER_LOGOUT")
frame:RegisterEvent("UPDATE_FACTION")
frame:RegisterEvent("MAJOR_FACTION_RENOWN_LEVEL_CHANGED")

frame:SetScript("OnEvent", function(_, event)
	if event == "PLAYER_ENTERING_WORLD" then
		if ready then
			return
		end
		-- The same eight seconds the collector waits. The faction list and the
		-- currency list are not populated the instant the world loads, and a
		-- snapshot taken too early is an empty baseline — which would credit
		-- this character with the account's entire history on the first fold.
		C_Timer.After(8, function()
			snapshot()
			C_Timer.NewTicker(FOLD_EVERY, fold)
		end)
		return
	end

	if event == "PLAYER_LOGOUT" then
		-- The important one. Everything since the last fold lands here.
		fold()
		return
	end

	-- A renown level or a standing changed. Folding on the event rather than
	-- only on the ticker means the numbers are right in the file even if the
	-- client is killed a moment later.
	fold()
end)

SLASH_ARMORYEARNED1 = "/armoryearned"
SlashCmdList["ARMORYEARNED"] = function()
	fold()
	local mine = db()
	if not mine then
		return
	end
	local factions, currencies = 0, 0
	for _ in pairs(mine.rep) do
		factions = factions + 1
	end
	for _ in pairs(mine.currency) do
		currencies = currencies + 1
	end
	print(string.format(
		"|cff8ab4f8Armory|r: this character has earned reputation with %d factions and %d currencies.",
		factions,
		currencies
	))
end

_ = ADDON
