-- Lint settings for the in-game addon.
--
-- The globals below are the World of Warcraft API. They are not declared
-- anywhere a linter can find, so without this every documented call the addon
-- makes reads as an undefined variable and the real warnings are lost in the
-- noise.
--
-- This replaced a `--globals` list on the luacheck command line in `test.sh`.
-- That list had drifted out of step with the addon twice, and a linter that is
-- only run on the machines that happen to have it installed is one that has to
-- be right the first time.

std = "lua51"
max_line_length = false

-- SavedVariables. The client creates these; the addon writes them.
globals = {
	"ArmoryCollectorDB",
	"ArmoryCollectorCharDB",
	"ArmoryChronicleDB",
	"SLASH_ARMORY1",
	"SLASH_ARMORYLOG1",
	"SLASH_ARMORYEARNED1",
	"SlashCmdList",
}

-- Everything the client provides, which the addon may read and never assign.
read_globals = {
	-- Added after a scope audit found them used and undeclared here. A
	-- linter that is noisy about real API calls hides the one warning that
	-- matters.
	"GetStatistic",
	"C_DeathInfo",
	"C_ToyBoxInfo",
	"C_DeathRecap",
	"UnitHealth",
	"UnitHealthMax",
	"UnitExists",
	"C_Map",
	"strsplit",
	"C_GossipInfo",
	"C_DelvesUI",
	"C_TalkingHead",
	"IsInCinematicScene",
	"GetNumSavedInstances",
	"GetSavedInstanceInfo",
	"C_ProfSpecs",
	"GetQuestID",

	-- Frames, timers and the Lua the client adds to the standard library.
	"CreateFrame",
	"C_Timer",
	"wipe",
	"time",
	"date",
	"print",

	-- Who and where.
	"UnitName",
	"UnitLevel",
	"UnitClass",
	"UnitRace",
	"UnitFactionGroup",
	"GetRealmName",
	"GetMoney",
	"GetGuildInfo",
	"GetAverageItemLevel",
	"GetSpecialization",
	"GetSpecializationInfo",
	"GetProfessions",
	"GetProfessionInfo",
	"GetZoneText",
	"GetRealZoneText",
	"GetSubZoneText",
	"GetNumGroupMembers",
	"IsInRaid",

	-- Achievements.
	"GetCategoryList",
	"GetCategoryInfo",
	"GetCategoryNumAchievements",
	"GetAchievementInfo",
	"GetAchievementNumCriteria",
	"GetAchievementCriteriaInfo",

	-- Journals and inventory.
	"C_MountJournal",
	"C_PetJournal",
	"C_ToyBox",
	"C_Item",
	"C_Container",
	"C_Bank",
	"C_CurrencyInfo",
	"PlayerHasToy",
	"Enum",

	-- Quests.
	"C_QuestLog",
	"C_CampaignInfo",
	"GetTitleText",
	"GetQuestText",
	"GetRewardText",
	"GetObjectiveText",

	-- Mail.
	"GetInboxNumItems",
	"GetInboxHeaderInfo",

	-- The chronicle: instances, the combat log, reputations, gear.
	"GetInstanceInfo",
	"C_ChallengeMode",
	"CombatLogGetCurrentEventInfo",
	"UnitGUID",
	"UnitClassification",
	"C_Reputation",
	"C_MajorFactions",
	"C_TransmogCollection",
	"GetInventoryItemLink",
	"Screenshot",
	"GetRepairAllCost",
	"C_TradeSkillUI",
}
