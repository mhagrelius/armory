-- Armory Collector
--
-- Records the things Blizzard's web API does not expose, so the Armory desktop
-- application can read them out of SavedVariables. Capture only: this addon
-- makes no decision, changes nothing, and automates nothing. It reads
-- documented APIs and writes two tables.
--
-- Two reasons this exists rather than Armory just asking Blizzard.
--
-- The first is data the web API simply does not have. `GetAchievementInfo`
-- returns `earnedBy` — which character originally earned an account-wide
-- achievement — and there is no equivalent field anywhere in the web API.
-- Armory uses it to decide whether a goal is *poisoned*: already earned, by
-- somebody outside the run's cohort, so its completion flag will never move
-- again however many times the content is replayed. Currencies, the Warband
-- bank and what each achievement criterion measures are the same story.
--
-- The second is that Blizzard's developer portal is unreliable, and an
-- application that cannot be used without it is an application that cannot be
-- used. Everything here is enough for Armory to work with no API client at all.
-- What it cannot replace is the auction house — `C_AuctionHouse.ReplicateItems`
-- is throttled to once every fifteen minutes and only covers the realm you are
-- standing on, so cross-realm pricing is not possible from in game.
--
-- Two files, on purpose. Account-wide data goes in `ArmoryCollectorDB` and
-- anything per character goes in `ArmoryCollectorCharDB`. A character's
-- completed-quest list is several thousand ids, and twenty-three of those in
-- one file would run at the Lua constant-table ceiling of 262,144 unique
-- literals — which does not fail gracefully, it fails to load.
--
-- WoW writes SavedVariables at logout or /reload and at no other time, so that
-- is when this runs. There is no way for an addon to open a socket or write a
-- file, and no need for one: Armory watches the folder.

local ADDON = ...

--- Bumped when the shape of either saved table changes. Armory refuses a
--- format it does not know rather than misreading it.
local FORMAT = 5

--- How many entries to walk per frame.
---
--- Scanning every achievement in the game in one go stalls the client for long
--- enough to be noticed, and this runs on login. A few hundred per frame
--- finishes in well under a second of wall time and never drops one.
local PER_FRAME = 300

local frame = CreateFrame("Frame")

--- Who we are, spelled the way Armory reads it back: `Name-Realm`, with the
--- realm's display name rather than its slug. Armory does the slug conversion,
--- in one place, because the game does not always agree with the API about
--- punctuation.
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
	store.format = FORMAT
	store.achievements = store.achievements or {}
	store.completed = store.completed or {}
	store.tree = store.tree or {}
	store.criteria = store.criteria or {}
	store.names = store.names or {}
	store.currencies = store.currencies or {}
	store.warbandBank = store.warbandBank or {}
	store.mounts = store.mounts or {}
	store.pets = store.pets or {}
	store.toys = store.toys or {}
	-- Which characters this addon has ever run on. Not read by Armory, which
	-- learns the roster from the per-character files — it is here for the
	-- chronicle, which needs to tell gold an alt mailed over from gold
	-- somebody else sent.
	store.roster = store.roster or {}
	local me = whoami()
	if me then
		store.roster[me] = true
	end
	return store
end

local function chardb()
	ArmoryCollectorCharDB = ArmoryCollectorCharDB or {}
	local store = ArmoryCollectorCharDB
	store.format = FORMAT
	return store
end

-- Achievements ---------------------------------------------------------------

--- The name of an achievement category, cached.
---
--- Every achievement in a category asks for the same title, and there are
--- thousands of achievements. It is also what tells a Feat of Strength from an
--- ordinary achievement, which decides whether a goal can ever be earned again.
--- Named `categoryNames` rather than `categories` because `scanAchievements`
--- has its own local list of category *ids* by that name, and one shadowing the
--- other is the sort of thing that works until somebody moves a line.
local categoryNames = {}
local function categoryTitle(categoryID)
	if categoryNames[categoryID] == nil then
		categoryNames[categoryID] = GetCategoryInfo(categoryID) or ""
	end
	return categoryNames[categoryID]
end

--- Record what each of an achievement's criteria measures, and which they are.
---
--- Two things at once, both of which the web API withholds. Blizzard's profile
--- response carries a criteria tree's *shape* and the account's progress
--- through it, and never says what any node measures — so a criterion knows it
--- needs 100 of something and not that the something is quests in Nagrand.
--- `GetAchievementCriteriaInfo` returns `criteriaType` and `assetID`, which is
--- exactly that mapping.
---
--- The list of criteria ids per achievement is kept as well, so Armory can
--- reconstruct the tree without the web API at all. Flat rather than nested:
--- the game gives a flat list, and every leaf is what observability turns on.
local function scanCriteria(store, achievementID)
	local count = GetAchievementNumCriteria(achievementID)
	if not count or count == 0 then
		return
	end

	local ids = {}
	for index = 1, count do
		local _, criteriaType, _, _, _, _, _, assetID, _, criteriaID =
			GetAchievementCriteriaInfo(achievementID, index)
		if criteriaID then
			ids[#ids + 1] = criteriaID
			-- An asset of 0 means the type does not use one, and storing it
			-- would claim a mapping that is not there.
			if criteriaType and assetID and assetID > 0 then
				store.criteria[criteriaID] = { criteriaType, assetID }
			end
		end
	end

	if #ids > 0 then
		store.tree[achievementID] = ids
	end
end

--- Walk every achievement: who earned it, when, and what it is made of.
---
--- Spread across frames with a ticker. `earnedBy` is nil for achievements
--- nobody has, and — this is the point — it names whoever earned it *first* and
--- goes on naming them forever. A second character completing the same content
--- produces no change here at all, which is precisely why Armory cannot track a
--- replay from completion flags and has to recompute from per-character data.
local function scanAchievements()
	local store = db()
	local categories = GetCategoryList()
	local category = 1
	local index = 1

	local function step()
		local budget = PER_FRAME
		while budget > 0 do
			local id = categories[category]
			if not id then
				store.writtenAt = time()
				return true
			end

			local total = GetCategoryNumAchievements(id, true)
			if index > total then
				category = category + 1
				index = 1
			else
				-- `id, name, points, completed, month, day, year, description,
				--  flags, icon, rewardText, isGuild, wasEarnedByMe, earnedBy`
				--
				-- An earlier version took only the id, the date and the
				-- attribution, and threw the rest away with underscores — which
				-- is why every achievement in the interface read "Achievement
				-- 42029". The web API has a catalogue endpoint for this, but it
				-- is one call per achievement across several thousand of them,
				-- and it needs a client the developer portal will not always
				-- issue. The game already knows.
				local achievementID, name, points, completed, month, day, year, description, _, icon, rewardText, _, wasEarnedByMe, earnedBy =
					GetAchievementInfo(id, index)

				if achievementID then
					if name then
						store.names[achievementID] = {
							name,
							points or 0,
							categoryTitle(id),
							description or "",
							rewardText or "",
							icon or 0,
						}
					end

					if completed then
						if wasEarnedByMe then
							store.achievements[achievementID] = whoami()
						elseif earnedBy then
							store.achievements[achievementID] = earnedBy
						end

						-- The date is what decides standing, so it matters more
						-- than it looks. `year` is years since 2000.
						if month and day and year then
							store.completed[achievementID] = time({
								year = 2000 + year,
								month = month,
								day = day,
								hour = 12,
							})
						else
							store.completed[achievementID] = true
						end
					end
					scanCriteria(store, achievementID)
				end
				index = index + 1
				budget = budget - 1
			end
		end
		return false
	end

	local ticker
	ticker = C_Timer.NewTicker(0, function()
		if step() then
			ticker:Cancel()
		end
	end)
end

-- Collections ----------------------------------------------------------------

--- Every mount, pet and toy: what exists, what is collected, and where from.
---
--- Better than the web API on the one axis that matters. `/data/wow/mount/{id}`
--- gives a one-word `source` type — `DROP` — with no NPC, no zone and no drop
--- rate, and pets have no source field at all. The journals carry `sourceText`,
--- which is the sentence Wowhead would show: "Drop: Attumen the Huntsman,
--- Karazhan". That is the difference between a list and an answer.
local function scanCollections()
	local store = db()

	for _, mountID in ipairs(C_MountJournal.GetMountIDs()) do
		-- `name, spellID, icon, isActive, isUsable, sourceType, isFavorite,
		--  isFactionSpecific, faction, shouldHideOnChar, isCollected, mountID`
		local name, spellID, icon, _, _, sourceType, _, factionSpecific, faction, _, collected =
			C_MountJournal.GetMountInfoByID(mountID)
		if name then
			-- `creatureDisplayInfoID, description, source, isSelfMount,
			--  mountTypeID, uiModelSceneID`
			local displayID, description, source, _, mountType =
				C_MountJournal.GetMountInfoExtraByID(mountID)

			store.mounts[mountID] = {
				name,
				collected and 1 or 0,
				source or "",
				sourceType or 0,
				-- The spell is what Wowhead indexes a mount under. Linking by
				-- the collection id lands on an unrelated page.
				spellID or 0,
				-- Flavour text. The web API has nothing like it, and an earlier
				-- version of this read it and threw it away.
				description or "",
				icon or 0,
				displayID or 0,
				mountType or 0,
				-- A mount one faction cannot use is not a gap in the other
				-- faction's collection, and saying "1321 missing" when a
				-- couple of hundred are unobtainable is a lie of omission.
				factionSpecific and (faction or 0) or -1,
			}
		end
	end

	-- Pets and toys are filter-sensitive: both journals answer with what the
	-- player's UI filters currently allow, so a scan sees the toy box and the
	-- pet journal as they are on screen rather than as they are.
	--
	-- **The filters are not cleared, and this comment used to say they were.**
	-- Clearing and restoring them is the documented dance, and it means an
	-- addon reaching into the journals' filter state on every logout — which
	-- 12.0 is exactly the patch to start refusing. What saves it is that these
	-- tables merge rather than replace: a filtered scan records fewer entries,
	-- never fewer than are already known. So this degrades to "a run with a
	-- search box open contributes less" rather than to a collection emptying.
	-- Mounts are unaffected; `GetMountIDs` ignores filters.
	local numPets = C_PetJournal.GetNumPets()
	for index = 1, numPets do
		local _, speciesID, owned = C_PetJournal.GetPetInfoByIndex(index)
		if speciesID then
			-- By species rather than by index, because the index form omits
			-- the flavour text and the creature id. `speciesName, speciesIcon,
			-- petType, companionID, tooltipSource, tooltipDescription, isWild,
			-- canBattle, isTradeable, isUnique, obtainable, creatureDisplayID`
			local name, icon, petType, companionID, source, description, _, _, isTradeable, _, _, displayID =
				C_PetJournal.GetPetInfoBySpeciesID(speciesID)

			-- How many of this species the journal holds. Nothing else answers
			-- "is this one a spare", and caging the only copy of a pet removes
			-- it from the collection — so a resale suggestion without this
			-- count is a suggestion to un-collect something.
			local held = C_PetJournal.GetNumCollectedInfo(speciesID) or 0

			if name then
				store.pets[speciesID] = {
					name,
					owned and 1 or 0,
					source or "",
					0,
					-- The creature, which is what Wowhead indexes a pet under.
					companionID or 0,
					description or "",
					icon or 0,
					displayID or 0,
					petType or 0,
					-1,
					-- Most pets cannot be caged at all. Without this the resale
					-- list is mostly things that cannot be sold.
					isTradeable and 1 or 0,
					held,
				}
			end
		end
	end

	-- `GetNumFilteredToys`, not `GetNumToys`. They are two different index
	-- spaces: the first counts every toy in the game, the second counts what
	-- the toy box is currently showing, and `GetToyFromIndex` indexes the
	-- second. Counting with one and indexing the other returns -1 for the
	-- overhang, which the guard below swallows — so the only sign of it is a
	-- collection quietly shorter than it should be.
	local filtered = C_ToyBox.GetNumFilteredToys and C_ToyBox.GetNumFilteredToys()
		or C_ToyBox.GetNumToys()
	for index = 1, filtered do
		local itemID = C_ToyBox.GetToyFromIndex(index)
		if itemID and itemID > 0 then
			-- `itemID, toyName, icon, isFavorite, hasFanfare, itemQuality`.
			-- There is no source text here, unlike the mount and pet journals
			-- — an earlier version read the sixth return as one and stored the
			-- item quality in the source field, which is why every toy came
			-- back sourceless. What the toy box does know is the item, and an
			-- item has a description worth having.
			local _, name, icon, _, _, quality = C_ToyBox.GetToyInfo(itemID)
			if name then
				store.toys[itemID] = {
					name,
					PlayerHasToy(itemID) and 1 or 0,
					"",
					0,
					itemID,
					-- No flavour text: the toy box knows the item and not its
					-- tooltip, and inventing an API that might exist is how a
					-- scan silently stops running. The Wowhead link is what
					-- answers "what does it do".
					"",
					icon or 0,
					0,
					quality or 0,
					-1,
				}
			end
		end
	end
end

-- Specialisations -------------------------------------------------------------

--- Which specialisation trees a profession has, and which are open.
---
--- A real progression system with no web API exposure at all: two characters
--- with Alchemy at 100 can have spent a year's knowledge in completely
--- different places, and nothing outside the game says so. Read from
--- `C_ProfSpecs`, which is keyed by skill line rather than by profession.
---
--- Returns a list of `{ name, unlocked }`, or an empty one for a profession
--- with no specialisations and for a client that predates them.
local function specialisations(skillLine)
	local out = {}
	if not skillLine or not C_ProfSpecs or not C_ProfSpecs.SkillLineHasSpecialization then
		return out
	end
	if not C_ProfSpecs.SkillLineHasSpecialization(skillLine) then
		return out
	end

	local configID = C_ProfSpecs.GetConfigIDForSkillLine(skillLine)
	local tabs = C_ProfSpecs.GetSpecTabIDsForSkillLine(skillLine) or {}
	for _, tabID in ipairs(tabs) do
		local info = C_ProfSpecs.GetTabInfo(tabID)
		if info and info.name then
			-- 0 Locked, 1 Unlocked, 2 Unlockable. Only "open" is recorded,
			-- because "could be opened" is a fact about the game rather than
			-- about this character.
			local state = configID and C_ProfSpecs.GetStateForTab(tabID, configID)
			out[#out + 1] = { info.name, state == 1 and 1 or 0 }
		end
	end
	return out
end

--- How much knowledge a profession has ever been given.
---
--- Knowledge left to spend on a specialisation tree.
---
--- `GetCurrencyInfoForSkillLine` returns a `SpecializationCurrencyInfo` — a
--- table of `numAvailable` and `currencyName` — and has done since 10.0.2. It
--- was read here as a currency id and handed to
--- `C_CurrencyInfo.GetCurrencyInfo`, which threw "bad argument #1 (outside of
--- expected range)" on every character with a specialised profession. See
--- `scanEverything` for what that cost.
---
--- **Unspent only.** The total ever earned is the figure that would measure a
--- year of weekly knowledge, and this API does not expose it — the currency
--- id that would reach `totalEarned` is not in the table. Reporting the
--- unspent amount is honest; inferring the total from it would not be.
local function knowledge(skillLine)
	if not skillLine or not C_ProfSpecs or not C_ProfSpecs.GetCurrencyInfoForSkillLine then
		return 0
	end
	local info = C_ProfSpecs.GetCurrencyInfoForSkillLine(skillLine)
	if type(info) ~= "table" then
		return 0
	end
	return info.numAvailable or 0
end

-- Recipes ---------------------------------------------------------------------

--- Which reagent slots are a cost rather than a choice.
---
--- Read off the enum where the client has it, with the documented value as the
--- fallback. An optional or finishing reagent is something a crafter *may*
--- add; pricing one as a cost would make every recipe look dearer than it is.
local BASIC_REAGENT = (Enum and Enum.CraftingReagentType and Enum.CraftingReagentType.Basic) or 1

--- Everything this character can make, with what it takes to make it.
---
--- The crafting tally beside this says what somebody has *made*. This says what
--- they *can* make, which is a different question and the one a flip needs —
--- and no endpoint answers either.
---
--- `GetAllRecipeIDs` returns an empty table until the profession window has
--- been opened, so this cannot run at login and there is no API that
--- substitutes. It is wired to the trade skill list instead: open the window
--- once on each character and the book is recorded. An empty answer is treated
--- as "not open yet" and leaves what was already stored alone, because a
--- character who has opened Alchemy and not Herbalism must keep their
--- Herbalism recipes.
---
--- `slot.reagents` is every quality tier of the same reagent, which is the
--- proof that tiers are separate item ids rather than variants of one: they are
--- commodities, and a commodity carries no bonus ids to vary by. All of them
--- are recorded so the desktop side can cost the cheapest one that has a price.
local function scanRecipes()
	if not C_TradeSkillUI or not C_TradeSkillUI.GetAllRecipeIDs then
		return
	end
	local me = whoami()
	if not me then
		return
	end

	local ids = C_TradeSkillUI.GetAllRecipeIDs()
	if not ids or #ids == 0 then
		return
	end

	local store = db()
	store.recipes = store.recipes or {}
	store.recipes[me] = store.recipes[me] or {}
	local mine = store.recipes[me]

	for _, id in ipairs(ids) do
		local info = C_TradeSkillUI.GetRecipeInfo(id)
		-- The list carries unlearnt recipes too, and a recipe nobody knows is
		-- not something this character can make.
		if info and info.learned and info.name and info.name ~= "" then
			local schematic = C_TradeSkillUI.GetRecipeSchematic(id, false)
			-- No output item means an enchant, a recraft or a transmute with
			-- nothing to sell, and there is no price to look up for it.
			if schematic and schematic.outputItemID then
				local reagents = {}
				for _, slot in ipairs(schematic.reagentSlotSchematics or {}) do
					if slot.required and slot.reagentType == BASIC_REAGENT and slot.reagents then
						local tiers = {}
						for _, reagent in ipairs(slot.reagents) do
							if reagent.itemID then
								tiers[#tiers + 1] = reagent.itemID
							end
						end
						if #tiers > 0 then
							reagents[#reagents + 1] = { slot.quantityRequired or 1, tiers }
						end
					end
				end
				if #reagents > 0 then
					mine[id] = {
						info.name,
						schematic.outputItemID,
						schematic.quantityMin or 1,
						reagents,
					}
				end
			end
		end
	end
end

--- One recipe, learned just now.
---
--- `scanRecipes` beside this needs the profession window; this does not, because
--- `GetRecipeSchematic` takes the id explicitly rather than reading whatever
--- list the window last loaded. Best-effort all the same — if the client
--- answers nothing for a recipe learned from a drop, the next time that window
--- is opened catches it, which is the same guarantee the rest of the book has.
local function learnRecipe(recipeID)
	if not recipeID or not C_TradeSkillUI or not C_TradeSkillUI.GetRecipeSchematic then
		return
	end
	local me = whoami()
	if not me then
		return
	end
	local info = C_TradeSkillUI.GetRecipeInfo(recipeID)
	local schematic = C_TradeSkillUI.GetRecipeSchematic(recipeID, false)
	if not info or not info.name or info.name == "" or not schematic or not schematic.outputItemID then
		return
	end

	local reagents = {}
	for _, slot in ipairs(schematic.reagentSlotSchematics or {}) do
		if slot.required and slot.reagentType == BASIC_REAGENT and slot.reagents then
			local tiers = {}
			for _, reagent in ipairs(slot.reagents) do
				if reagent.itemID then
					tiers[#tiers + 1] = reagent.itemID
				end
			end
			if #tiers > 0 then
				reagents[#reagents + 1] = { slot.quantityRequired or 1, tiers }
			end
		end
	end
	if #reagents == 0 then
		return
	end

	local store = db()
	store.recipes = store.recipes or {}
	store.recipes[me] = store.recipes[me] or {}
	store.recipes[me][recipeID] = {
		info.name,
		schematic.outputItemID,
		schematic.quantityMin or 1,
		reagents,
	}
end

-- Currencies -----------------------------------------------------------------

--- Every currency this character holds.
---
--- No web endpoint returns these — not Trader's Tender, not crests, not
--- valorstones.
local function scanCurrencies()
	local me = whoami()
	if not me then
		return
	end

	local store = db()
	local mine = {}

	for index = 1, C_CurrencyInfo.GetCurrencyListSize() do
		local entry = C_CurrencyInfo.GetCurrencyListInfo(index)
		if entry and not entry.isHeader then
			local link = C_CurrencyInfo.GetCurrencyListLink(index)
			local id = link and tonumber(link:match("currency:(%d+)"))
			if id then
				mine[id] = entry.quantity or 0
			end
		end
	end

	store.currencies[me] = mine
end

-- The Warband bank -----------------------------------------------------------

--- What is in the account bank.
---
--- Added in 11.0 as `Enum.BankType.Account`, with tabs read through the same
--- `C_Container` calls as any other bag. There is no endpoint for this and
--- Blizzard has said there will not be one.
local function scanWarbandBank()
	if not C_Bank or not Enum.BankType or not Enum.BankType.Account then
		return
	end
	if not C_Bank.CanViewBank(Enum.BankType.Account) then
		-- Not at a banker. The last scan stands rather than being replaced by
		-- an empty one.
		return
	end

	local store = db()
	local contents = {}

	local tabs = C_Bank.FetchPurchasedBankTabData(Enum.BankType.Account) or {}
	for _, tab in ipairs(tabs) do
		local bag = tab.ID
		for slot = 1, C_Container.GetContainerNumSlots(bag) do
			local item = C_Container.GetContainerItemInfo(bag, slot)
			if item and item.itemID then
				contents[item.itemID] = (contents[item.itemID] or 0) + (item.stackCount or 1)
			end
		end
	end

	store.warbandBank = contents
	store.warbandMoney = C_Bank.FetchDepositedMoney(Enum.BankType.Account)
end

-- This character -------------------------------------------------------------

--- The inventory slot ids, paired with the names the web API uses for them.
---
--- Written out rather than derived, because the two id spaces have to be joined
--- somewhere and this is the only place that knows both. The names are
--- Blizzard's own `slot.type` from `/profile/.../equipment`, so a character read
--- off the addon and one read off the API produce the same rows and the reader
--- does not have to care which it got.
---
--- Shirt and tabard are here and carry no item level, the same as the API: they
--- are worn and they are not gear.
local SLOTS = {
	{ 1, "HEAD" },
	{ 2, "NECK" },
	{ 3, "SHOULDER" },
	{ 4, "SHIRT" },
	{ 5, "CHEST" },
	{ 6, "WAIST" },
	{ 7, "LEGS" },
	{ 8, "FEET" },
	{ 9, "WRIST" },
	{ 10, "HANDS" },
	{ 11, "FINGER_1" },
	{ 12, "FINGER_2" },
	{ 13, "TRINKET_1" },
	{ 14, "TRINKET_2" },
	{ 15, "BACK" },
	{ 16, "MAIN_HAND" },
	{ 17, "OFF_HAND" },
	{ 19, "TABARD" },
}

--- What is worn, slot by slot.
---
--- Only slots that hold something, which is the same shape the web API answers
--- in: an empty slot is an absent row rather than a row saying nothing. The
--- reader is what turns absence back into "the off hand is empty", and it can
--- only do that if absence means absence.
---
--- The item level is `C_Item.GetItemInfo`'s fourth return, which is the
--- *effective* level — what the item is actually worth on this character rather
--- than its base. A shirt answers one, and one is not an item level, so the
--- cosmetic slots are written with an empty string and the reader turns that
--- back into nothing. Empty string rather than nil for the reason the whole of
--- `Chronicle.lua` uses it: WoW's serializer writes a table with an interior
--- hole as keyed entries, and the row would come back a different shape.
local function equipment()
	local worn = {}
	for _, slot in ipairs(SLOTS) do
		local index, name = slot[1], slot[2]
		local link = GetInventoryItemLink("player", index)
		if link then
			-- `GetDetailedItemLevelInfo`, not `GetItemInfo`. The fourth
			-- return of `GetItemInfo` is the *base* level, before upgrades —
			-- and a whole Veteran-to-Myth track shares one base item, so a
			-- fully upgraded piece and the drop it came from record
			-- identically. Sorting the character page weakest-slot-first on
			-- that number sorts on something that does not vary with the
			-- thing it is about.
			local level = C_Item.GetDetailedItemLevelInfo(link)
			local title = C_Item.GetItemInfo(link)
			local cosmetic = name == "SHIRT" or name == "TABARD"
			worn[#worn + 1] = {
				name,
				title or "",
				(not cosmetic) and (level or 0) or "",
			}
		end
	end
	return worn
end

--- This week's raid lockouts.
---
--- **Not the same fact as the web API's raid progress, and not merged with it.**
--- The API reports every boss this character has ever killed; the client knows
--- only what it is saved to, which is this week. A lockout is still worth
--- having — it is the only raid progress an addon-only account has at all, and
--- for the tier being raided now it is the more current of the two — but
--- calling it lifetime progress would be inventing a decade of raiding out of
--- one reset.
---
--- `GetSavedInstanceInfo` answers extended and expired lockouts too, so the
--- ones that have run out are dropped rather than shown as this week's.
local function raidLocks()
	local locks = {}
	for index = 1, GetNumSavedInstances() do
		local name, _, reset, difficulty, locked, _, _, isRaid, _, difficultyName, encounters, defeated =
			GetSavedInstanceInfo(index)
		if isRaid and locked and (reset or 0) > 0 then
			locks[#locks + 1] = {
				name or "",
				difficultyName or "",
				defeated or 0,
				encounters or 0,
				difficulty or 0,
			}
		end
	end
	return locks
end

--- Everything about the character being played.
---
--- Goes in the per-character file. The completed-quest list alone is several
--- thousand ids, and twenty-three of those in the account file would run at the
--- Lua constant-table ceiling — which does not fail gracefully.
---
--- With this, Armory needs no web API to know a roster: every character you log
--- in on describes itself. The cost is that a character you never log in on is
--- one Armory never hears about, which is the trade the API would otherwise
--- solve.
local function scanCharacter()
	local me = whoami()
	if not me then
		return
	end

	local store = chardb()

	--- Keep the better of what we have and what we just read.
	---
	--- This runs twice: eight seconds after login, when everything is
	--- available, and again at logout, when almost nothing is. At logout
	--- `GetMoney` answers 0, `GetAverageItemLevel` answers 0,
	--- `GetAllCompletedQuestIDs` answers an empty table and `GetProfessions`
	--- answers nil — so a plain assignment at logout wipes out everything the
	--- login scan found. That is exactly what a first version of this did, and
	--- the file it produced described a level 90 character with no gear, no
	--- gold, no professions and no quests.
	---
	--- So: never replace something with nothing.
	local function keep(field, value)
		if value == nil then
			return
		end
		if type(value) == "number" and value == 0 and (store[field] or 0) > 0 then
			return
		end
		if type(value) == "table" and #value == 0 and store[field] and #store[field] > 0 then
			return
		end
		store[field] = value
	end

	local _, class = UnitClass("player")
	local _, race = UnitRace("player")
	local specIndex = GetSpecialization()
	local specName = nil
	if specIndex then
		_, specName = GetSpecializationInfo(specIndex)
	end

	-- Identity is always available and never wrong, so it is assigned rather
	-- than kept.
	store.name = UnitName("player")
	store.realm = GetRealmName()
	store.class = class
	store.race = race
	store.faction = UnitFactionGroup("player")

	keep("level", UnitLevel("player"))
	keep("money", GetMoney())
	keep("spec", specName)
	keep("guild", (GetGuildInfo("player")))
	-- Parenthesised for the same reason the quest giver is: `select(2, …)`
	-- expands to every remaining return, so this passes `keep` three
	-- arguments and works only because it ignores the third.
	keep("itemLevel", (select(2, GetAverageItemLevel())))
	-- Genuinely per character, and the single most useful thing here: a
	-- replayed character's quest log grows even when the account-wide
	-- achievement it feeds has been lit for a decade.
	keep("quests", C_QuestLog.GetAllCompletedQuestIDs())

	local professions = {}
	local first, second, _, fishing, cooking = GetProfessions()
	for _, slot in ipairs({ first, second, fishing, cooking }) do
		if slot then
			-- The seventh return is the skill line, which is what every
			-- specialisation call is keyed by. `GetProfessionInfo` takes the
			-- *index* from `GetProfessions` and not a profession id.
			local name, _, rank, maxRank, _, _, skillLine = GetProfessionInfo(slot)
			if name then
				-- Coerced, never passed through. If either of these ever
				-- answered nil the row would carry an interior hole, and
				-- WoW's serializer writes a table with a hole as *keyed*
				-- entries rather than as a padded array — so the row would
				-- come back to Armory as four elements and the two new
				-- columns would vanish without any error anywhere. That trap
				-- is written down for `Chronicle.lua` and applies here just
				-- as much.
				local trees = specialisations(skillLine) or {}
				professions[#professions + 1] = {
					name,
					rank or 0,
					maxRank or 0,
					(slot == first or slot == second) and 1 or 0,
					trees,
					knowledge(skillLine) or 0,
				}
			end
		end
	end
	keep("professions", professions)
	keep("equipment", equipment())
	keep("raidLocks", raidLocks())

	store.scannedAt = time()
end

-- Wiring ---------------------------------------------------------------------

--- Everything the collector reads, each step on its own.
---
--- **Guarded individually, and that guard is the whole point.** These ran as
--- five bare calls until one of them threw: `knowledge` handed a table to an
--- API expecting a number, `scanCharacter` died at that line, and every step
--- after it — the currencies, the collections, the Warband bank, the whole
--- achievement catalogue — never ran at all on any character with a
--- specialised profession. It also took the professions, the equipment and
--- the raid locks with it, because those are written after the loop that
--- failed.
---
--- Nothing said so. The client hides Lua errors by default, and an account
--- read that silently stops half way looks exactly like an account with less
--- in it. The Warband bank reading empty was blamed on unverified bag indices
--- for weeks; it was this.
---
--- So a step that throws now costs that step. The same rule the event
--- registration already follows: a patch renaming one thing should not take
--- everything else with it.
local function scanEverything()
	for _, step in ipairs({
		{ "character", scanCharacter },
		{ "currencies", scanCurrencies },
		{ "collections", scanCollections },
		{ "warband bank", scanWarbandBank },
		{ "achievements", scanAchievements },
	}) do
		local ok, err = pcall(step[2])
		if not ok then
			ArmoryCollectorDB = ArmoryCollectorDB or {}
			local broke = ArmoryCollectorDB.broke or {}
			broke[step[1]] = tostring(err)
			ArmoryCollectorDB.broke = broke
		end
	end
end

frame:RegisterEvent("PLAYER_ENTERING_WORLD")
frame:RegisterEvent("PLAYER_LOGOUT")
frame:RegisterEvent("BANKFRAME_OPENED")
frame:RegisterEvent("CURRENCY_DISPLAY_UPDATE")
-- The recipe book cannot be read at login: `GetAllRecipeIDs` answers an empty
-- table until the profession window has been opened, and there is no API that
-- substitutes. So it is read from the window itself.
frame:RegisterEvent("TRADE_SKILL_LIST_UPDATE")
frame:RegisterEvent("TRADE_SKILL_SHOW")
-- And every recipe learned after that, so the book stays current without
-- anybody having to remember to open a window again.
frame:RegisterEvent("NEW_RECIPE_LEARNED")

frame:SetScript("OnEvent", function(_, event, ...)
	if event == "PLAYER_ENTERING_WORLD" then
		-- A short delay: the journals and the currency list are not populated
		-- the instant the world loads, and a scan that runs too early records
		-- an empty account.
		C_Timer.After(8, scanEverything)
	elseif event == "PLAYER_LOGOUT" then
		-- The last chance to write. The achievement walk is deliberately not
		-- started here — a ticker cannot finish during logout — so what gets
		-- saved is whatever the login scan collected, which is the same data.
		scanCharacter()
		scanCurrencies()
		db().writtenAt = time()
	elseif event == "BANKFRAME_OPENED" then
		C_Timer.After(1, scanWarbandBank)
	elseif event == "CURRENCY_DISPLAY_UPDATE" then
		scanCurrencies()
	elseif event == "NEW_RECIPE_LEARNED" then
		learnRecipe(...)
	elseif event == "TRADE_SKILL_LIST_UPDATE" or event == "TRADE_SKILL_SHOW" then
		-- Both, because the list arrives after the frame on a cold open and
		-- the frame is already up on a warm one. Scanning twice is cheap and
		-- scanning neither loses the whole book.
		scanRecipes()
	end
end)

SLASH_ARMORY1 = "/armory"
SlashCmdList["ARMORY"] = function()
	scanEverything()
	print("|cff8ab4f8Armory|r: scanning. Log out or /reload to write the file.")
end

_ = ADDON
