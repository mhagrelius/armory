using System.Text;
using Armory.Blizzard;
using Armory.Collections;
using Armory.Roster;
using Armory.Zones;
using Xunit;

namespace Armory.Tests.Blizzard;

/// <summary>Ported from <c>collections.rs</c>, <c>gamedata.rs</c>, <c>media.rs</c> and <c>auctions.rs</c>, fixtures verbatim.</summary>
public sealed class CatalogueTests
{
    private static byte[] B(string text) => Encoding.UTF8.GetBytes(text);

    private static T Found<T>(Outcome<T> outcome) => Assert.IsType<Outcome<T>.Found>(outcome).Value;

    private static Collectible Bare(Kind kind, long id, string name) => new() { Kind = kind, Id = id, Name = name, LinkId = id };

    // -- collections ---------------------------------------------------------------

    [Fact(DisplayName = "decor_is_a_collection_like_any_other")]
    public void Decor_is_a_collection_like_any_other()
    {
        Assert.Contains("/data/wow/decor/index", CollectionsApi.Index(Region.Us, Kind.Decor).Url, StringComparison.Ordinal);
        Assert.Contains("/data/wow/decor/42", CollectionsApi.Detail(Region.Us, Kind.Decor, 42).Url, StringComparison.Ordinal);
        Assert.Contains("/profile/user/wow/collections/decor", CollectionsApi.Collected(Region.Us, Kind.Decor).Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_decor_index_calls_its_list_decor_items")]
    public void The_decor_index_calls_its_list_decor_items()
    {
        var catalogue = Found(CollectionsApi.ParseIndex(B("""{"_links":{},"decor_items":[{"key":{"href":"x"},"name":"Lorewalker's Bookcase","id":300},{"key":{"href":"y"},"name":"Zandalari War Torch","id":301}]}"""), Kind.Decor));
        Assert.Equal(2, catalogue.Count);
        Assert.Equal("Lorewalker's Bookcase", catalogue[0].Name);
        Assert.Equal(Kind.Decor, catalogue[0].Kind);
    }

    [Fact(DisplayName = "a_collectible_with_no_item_yet_is_not_linked_at_all")]
    public void A_collectible_with_no_item_yet_is_not_linked_at_all()
    {
        var chair = Bare(Kind.Decor, 5, "Sturdy Chair");
        Assert.True(chair.ItemIdIsGuessed());
        Assert.Null(chair.WowheadUrl());
        Assert.Null(chair.KnownItemId());

        chair = chair with { LinkId = 246_810 };
        Assert.False(chair.ItemIdIsGuessed());
        Assert.Equal(246_810, chair.KnownItemId());
        Assert.Equal("https://www.wowhead.com/item=246810", chair.WowheadUrl());
    }

    [Fact(DisplayName = "a_piece_of_decor_links_by_the_item_it_is")]
    public void A_piece_of_decor_links_by_the_item_it_is()
    {
        var entry = Found(CollectionsApi.ParseDetail(B("""{"id":300,"name":"Lorewalker's Bookcase","item":{"key":{"href":"z"},"name":"Lorewalker's Bookcase","id":246810},"source":{"type":"VENDOR","name":"Vendor"}}"""), Kind.Decor));
        Assert.Equal(300, entry.Id);
        Assert.Equal(246810, entry.LinkId);
        Assert.Equal(Source.Vendor, entry.Source);
        Assert.Equal("https://www.wowhead.com/item=246810", entry.WowheadUrl());
    }

    [Fact(DisplayName = "owned_decor_is_read_whether_the_id_is_nested_or_not")]
    public void Owned_decor_is_read_whether_the_id_is_nested_or_not()
    {
        Assert.Equal([300L], Found(CollectionsApi.ParseCollected(B("""{"decor":[{"decor":{"name":"Bookcase","id":300}}]}"""), Kind.Decor)));
        Assert.Equal([300L], Found(CollectionsApi.ParseCollected(B("""{"decor":[{"id":300,"quantity":4}]}"""), Kind.Decor)));
    }

    [Fact(DisplayName = "each_collection_nests_its_ids_differently")]
    public void Each_collection_nests_its_ids_differently()
    {
        Assert.Equal(2, Found(CollectionsApi.ParseCollected(B("""{"mounts":[{"mount":{"id":6}},{"mount":{"id":7}}]}"""), Kind.Mount)).Count);
        Assert.Contains(42L, Found(CollectionsApi.ParseCollected(B("""{"pets":[{"species":{"id":42}}]}"""), Kind.Pet)));
        Assert.Contains(9L, Found(CollectionsApi.ParseCollected(B("""{"toys":[{"toy":{"id":9}}]}"""), Kind.Toy)));
    }

    [Fact(DisplayName = "a_missing_list_is_stale_rather_than_an_empty_collection")]
    public void A_missing_list_is_stale_rather_than_an_empty_collection()
    {
        Assert.IsType<Outcome<HashSet<long>>.Stale>(CollectionsApi.ParseCollected(B("""{"character":{}}"""), Kind.Mount));
    }

    [Fact(DisplayName = "an_index_yields_names_but_never_sources")]
    public void An_index_yields_names_but_never_sources()
    {
        var mounts = Found(CollectionsApi.ParseIndex(B("""{"mounts":[{"id":6,"name":"Brown Horse"},{"id":7,"name":"Grey Ram"}]}"""), Kind.Mount));
        Assert.Equal(2, mounts.Count);
        Assert.Equal("Brown Horse", mounts[0].Name);
        Assert.Equal(Source.Unknown, mounts[0].Source);
    }

    [Fact(DisplayName = "a_mount_links_by_its_spell_rather_than_its_collection_id")]
    public void A_mount_links_by_its_spell_rather_than_its_collection_id()
    {
        var mount = Found(CollectionsApi.ParseDetail(B("""{"id":6,"name":"Brown Horse","source":{"type":"VENDOR"},"source_spell":{"id":458}}"""), Kind.Mount));
        Assert.Equal(458, mount.LinkId);
        Assert.Equal("https://www.wowhead.com/spell=458", mount.WowheadUrl());
        Assert.Equal(Source.Vendor, mount.Source);
    }

    [Fact(DisplayName = "a_mount_with_no_spell_still_gets_a_link")]
    public void A_mount_with_no_spell_still_gets_a_link()
    {
        Assert.Equal(6, Found(CollectionsApi.ParseDetail(B("""{"id":6,"name":"Old"}"""), Kind.Mount)).LinkId);
    }

    [Fact(DisplayName = "a_missing_source_is_unknown_rather_than_assumed")]
    public void A_missing_source_is_unknown_rather_than_assumed()
    {
        Assert.Equal(Source.Unknown, Found(CollectionsApi.ParseDetail(B("""{"id":6,"name":"Old"}"""), Kind.Mount)).Source);
    }

    [Fact(DisplayName = "a_promotional_mount_can_never_be_earned_again")]
    public void A_promotional_mount_can_never_be_earned_again()
    {
        Assert.False(Source.Promotion.IsRepeatable());
        Assert.True(Source.Drop.IsRepeatable());
        Assert.True(Source.Unknown.IsRepeatable());
    }

    [Fact(DisplayName = "collections_are_asked_for_at_the_account_and_not_per_character")]
    public void Collections_are_asked_for_at_the_account_and_not_per_character()
    {
        var request = CollectionsApi.Collected(Region.Us, Kind.Mount);
        Assert.Contains("/profile/user/wow/collections/mounts", request.Url, StringComparison.Ordinal);
        Assert.DoesNotContain("/profile/wow/character/", request.Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_catalogue_is_static_and_the_collection_is_profile")]
    public void The_catalogue_is_static_and_the_collection_is_profile()
    {
        Assert.Contains("namespace=static-us", CollectionsApi.Index(Region.Us, Kind.Pet).Url, StringComparison.Ordinal);
        Assert.Contains("namespace=profile-us", CollectionsApi.Collected(Region.Us, Kind.Pet).Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a faction-locked mount is not missing from a collection that could never hold it")]
    public void A_faction_locked_mount_is_not_missing_from_a_collection_that_could_never_hold_it()
    {
        var horde = Bare(Kind.Mount, 1, "Kor'kron") with { Faction = Faction.Horde, Source = Source.Drop };
        Assert.True(horde.ObtainableBy(Faction.Horde));
        Assert.False(horde.ObtainableBy(Faction.Alliance));
        Assert.False((horde with { Faction = null, Source = Source.Promotion }).ObtainableBy(Faction.Horde));
    }

    // -- game data -----------------------------------------------------------------

    [Fact(DisplayName = "an_achievement_is_read_whole")]
    public void An_achievement_is_read_whole()
    {
        var achievement = Found(GameData.ParseAchievement(B("""{"id": 4956, "name": "Loremaster of Kalimdor", "points": 50,"description": "Complete the Kalimdor quest achievements.","category": {"id": 97, "name": "Quests"}}""")));
        Assert.Equal(4956, achievement.Id);
        Assert.Equal("Loremaster of Kalimdor", achievement.Name);
        Assert.Equal(50, achievement.Points);
        Assert.Equal("Quests", achievement.Category);
        Assert.False(achievement.IsUnrepeatable);
    }

    [Fact(DisplayName = "a_feat_of_strength_is_flagged_as_unrepeatable")]
    public void A_feat_of_strength_is_flagged_as_unrepeatable()
    {
        Assert.True(Found(GameData.ParseAchievement(B("""{"id": 1, "name": "Gone", "category": {"name": "Feats of Strength"}}"""))).IsUnrepeatable);
    }

    [Fact(DisplayName = "a_search_result_carries_every_locales_name_at_once")]
    public void A_search_result_carries_every_locales_name_at_once()
    {
        var body = B("""{"page":1,"results":[{"data":{"id":197794,"name":{"en_US":"Mycobloom","de_DE":"Pilzbluete"}}},{"data":{"id":210796,"name":{"en_US":"Crystalline Powder"}}}]}""");
        var found = Found(GameData.ParseItemSearch(body, "en_US"));
        Assert.Equal((197794L, "Mycobloom"), found[0]);
        Assert.Equal(210796, found[1].Id);
        Assert.Equal("Mycobloom", Found(GameData.ParseItemSearch(body, "ko_KR"))[0].Name);
    }

    [Fact(DisplayName = "a_search_names_the_locale_it_is_searching_in")]
    public void A_search_names_the_locale_it_is_searching_in()
    {
        Assert.Contains("name.en_US=Mycobloom", GameData.ItemSearch(Region.Us, "Mycobloom").Url, StringComparison.Ordinal);
        Assert.Contains("name.en_GB=", GameData.ItemSearch(Region.Eu, "x").Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "the_catalogue_is_asked_for_in_the_static_namespace")]
    public void The_catalogue_is_asked_for_in_the_static_namespace()
    {
        Assert.Contains("namespace=static-us", GameData.AchievementOf(Region.Us, 4956).Url, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "an item's binding decides whether it can be sold and silence is a yes")]
    public void An_items_binding_decides_whether_it_can_be_sold_and_silence_is_a_yes()
    {
        var bound = Found(GameData.ParseItem(B("""{"id":1,"name":"Helm","quality":{"type":"EPIC"},"preview_item":{"binding":{"type":"ON_ACQUIRE"}}}""")));
        Assert.False(bound.Sellable);
        Assert.Equal("EPIC", bound.Quality);
        var free = Found(GameData.ParseItem(B("""{"id":2,"name":"Mycobloom"}""")));
        Assert.True(free.Sellable);
        Assert.IsType<Outcome<Item>.Stale>(GameData.ParseItem(B("""{"id":3,"name":""}""")));
        Assert.Equal("Mycobloom", Found(GameData.ParseItemName(B("""{"id":2,"name":"Mycobloom"}"""))));
    }

    [Fact(DisplayName = "an instance and an encounter read into the guide's records")]
    public void An_instance_and_an_encounter_read_into_the_guides_records()
    {
        var instance = Found(GameData.ParseInstance(B("""{"id":745,"name":"Karazhan","map":{"id":350},"description":"A tower.","category":{"type":"RAID"},"encounters":[{"id":1},{"id":2},{"id":1}]}""")));
        Assert.Equal(new Instance { Id = 745, Name = "Karazhan", Map = 350, Description = "A tower.", Expansion = "RAID", Encounters = [1, 2, 1] }, instance);
        var encounter = Found(GameData.ParseEncounter(B("""{"id":1,"name":"Attumen","description":"A horse.","items":[{"id":99,"item":{"id":30480}}]}""")));
        Assert.Equal(new Encounter { Id = 1, Name = "Attumen", Description = "A horse.", Loot = [30480] }, encounter);
        Assert.Equal([(745L, "Karazhan")], Found(GameData.ParseInstanceIndex(B("""{"instances":[{"id":745,"name":"Karazhan"}]}"""))));
        Assert.Equal([1L, 2L], Found(GameData.ParseAchievementIndex(B("""{"achievements":[{"id":1},{"id":2}]}"""))));
    }

    // -- media ---------------------------------------------------------------------

    [Fact(DisplayName = "a_creature_render_is_addressed_by_display_id_and_costs_nothing")]
    public void A_creature_render_is_addressed_by_display_id_and_costs_nothing()
    {
        Assert.Equal("https://render.worldofwarcraft.com/us/npcs/zoom/creature-display-2404.jpg", Media.CreatureRender(Region.Us, 2404));
        Assert.StartsWith("https://render.worldofwarcraft.com/eu/", Media.CreatureRender(Region.Eu, 1), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_class_crest_is_its_name_with_the_spaces_taken_out")]
    public void A_class_crest_is_its_name_with_the_spaces_taken_out()
    {
        Assert.EndsWith("classicon_deathknight.jpg", Media.ClassIcon(Region.Us, "Death Knight"), StringComparison.Ordinal);
        Assert.EndsWith("classicon_demonhunter.jpg", Media.ClassIcon(Region.Us, "Demon Hunter"), StringComparison.Ordinal);
        Assert.EndsWith("classicon_evoker.jpg", Media.ClassIcon(Region.Us, "Evoker"), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "neutral_has_no_crest_because_it_is_not_a_side")]
    public void Neutral_has_no_crest_because_it_is_not_a_side()
    {
        Assert.NotNull(Media.FactionIcon(Region.Us, Faction.Horde));
        Assert.NotNull(Media.FactionIcon(Region.Us, Faction.Alliance));
        Assert.Null(Media.FactionIcon(Region.Us, Faction.Neutral));
    }

    [Fact(DisplayName = "an_asset_is_found_by_key_rather_than_by_position")]
    public void An_asset_is_found_by_key_rather_than_by_position()
    {
        var body = B("""{"assets":[{"key":"inset","value":"https://render/inset.jpg"},{"key":"avatar","value":"https://render/avatar.jpg"},{"key":"main-raw","value":"https://render/main-raw.png"}]}""");
        Assert.Equal("https://render/avatar.jpg", Found(Media.ParsePortrait(body, Portrait.Avatar)));
        Assert.Equal("https://render/main-raw.png", Found(Media.ParsePortrait(body, Portrait.Main)));
    }

    [Fact(DisplayName = "a_portrait_asks_for_the_raw_render_not_the_composited_one")]
    public void A_portrait_asks_for_the_raw_render_not_the_composited_one()
    {
        Assert.Equal("main-raw", Portrait.Main.Key());
    }

    [Fact(DisplayName = "an_entry_with_no_art_is_empty_rather_than_broken")]
    public void An_entry_with_no_art_is_empty_rather_than_broken()
    {
        Assert.IsType<Outcome<string>.Empty>(Media.ParseIcon(B("""{"assets":[]}""")));
        Assert.IsType<Outcome<string>.Empty>(Media.ParseIcon(B("""{"assets":[{"key":"zoom","value":"x"}]}""")));
    }

    [Fact(DisplayName = "a_media_url_gives_back_the_id_it_was_built_for")]
    public void A_media_url_gives_back_the_id_it_was_built_for()
    {
        Assert.Equal(86571, Media.MediaId(Media.Item(Region.Us, 86571).Url, Media.ItemMedia));
        Assert.Equal(4956, Media.MediaId(Media.Achievement(Region.Eu, 4956).Url, Media.AchievementMedia));
        Assert.Null(Media.MediaId(Media.Item(Region.Us, 86571).Url, Media.AchievementMedia));
        Assert.Null(Media.MediaId("https://us.api.blizzard.com/data/wow/toy/1", Media.ItemMedia));
    }

    [Fact(DisplayName = "item_media_is_static_and_character_media_is_profile")]
    public void Item_media_is_static_and_character_media_is_profile()
    {
        Assert.Contains("namespace=static-us", Media.Item(Region.Us, 32566).Url, StringComparison.Ordinal);
        Assert.Contains("namespace=static-us", Media.Achievement(Region.Us, 4956).Url, StringComparison.Ordinal);
        var request = Media.Character(Region.Us, new CharacterKey("mannoroth", "Aeltor"));
        Assert.Contains("namespace=profile-us", request.Url, StringComparison.Ordinal);
        Assert.Contains("/profile/wow/character/mannoroth/aeltor/character-media", request.Url, StringComparison.Ordinal);
    }

    // -- auctions ------------------------------------------------------------------

    private static Listing Listed(long itemId, long price, long quantity, string variant = "") =>
        new() { ItemId = itemId, UnitPrice = price, Quantity = quantity, Variant = variant };

    [Fact(DisplayName = "the_index_gives_hrefs_and_the_id_is_read_off_the_end")]
    public void The_index_gives_hrefs_and_the_id_is_read_off_the_end()
    {
        Assert.Equal([61L, 3684L], Found(Auctions.ParseConnectedRealmIndex(B("""{"connected_realms":[{"href":"https://us.api.blizzard.com/data/wow/connected-realm/61?namespace=dynamic-us"},{"href":"https://us.api.blizzard.com/data/wow/connected-realm/3684?namespace=dynamic-us"}]}"""))));
    }

    [Fact(DisplayName = "the_realm_index_comes_back_alphabetical_not_in_the_order_they_opened")]
    public void The_realm_index_comes_back_alphabetical_not_in_the_order_they_opened()
    {
        var realms = Found(Auctions.ParseRealmIndex(B("""{"realms":[{"id":61,"name":"Mannoroth","slug":"mannoroth"},{"id":1567,"name":"Emerald Dream","slug":"emerald-dream"}]}""")));
        Assert.Equal("Emerald Dream", realms[0].Name);
        Assert.Equal("emerald-dream", realms[0].Slug);
        Assert.Equal("Mannoroth", realms[1].Name);
    }

    [Fact(DisplayName = "a_realm_says_which_auction_house_it_trades_in")]
    public void A_realm_says_which_auction_house_it_trades_in()
    {
        Assert.Equal(61, Found(Auctions.ParseRealmConnection(B("""{"id":1567,"name":"Terenas","connected_realm":{"href":"https://us.api.blizzard.com/data/wow/connected-realm/61?namespace=dynamic-us"}}"""))));
    }

    [Fact(DisplayName = "a_realm_with_no_connection_is_stale_rather_than_realm_zero")]
    public void A_realm_with_no_connection_is_stale_rather_than_realm_zero()
    {
        Assert.IsType<Outcome<long>.Stale>(Auctions.ParseRealmConnection(B("""{"id":1567,"name":"Terenas"}""")));
    }

    [Fact(DisplayName = "a_connected_realm_names_every_realm_that_shares_its_auction_house")]
    public void A_connected_realm_names_every_realm_that_shares_its_auction_house()
    {
        var realm = Found(Auctions.ParseConnectedRealm(B("""{"id":61,"realms":[{"id":61,"name":"Emerald Dream","slug":"emerald-dream"},{"id":1567,"name":"Terenas","slug":"terenas"}]}""")));
        Assert.Equal(61, realm.Id);
        Assert.Equal(["Emerald Dream", "Terenas"], realm.Realms);
        Assert.Equal(["emerald-dream", "terenas"], realm.Slugs);
    }

    [Fact(DisplayName = "commodities_carry_a_unit_price_and_nothing_else")]
    public void Commodities_carry_a_unit_price_and_nothing_else()
    {
        var listings = Found(Auctions.ParseCommodities(B("""{"auctions":[{"id":1,"item":{"id":197794},"quantity":20,"unit_price":56523,"time_left":"SHORT"}]}""")));
        Assert.Equal(197794, listings[0].ItemId);
        Assert.Equal(56523, listings[0].UnitPrice);
        Assert.Equal("", listings[0].Variant);
        Assert.Null(listings[0].PetSpecies);
    }

    [Fact(DisplayName = "a_realm_auction_prices_per_unit_from_its_buyout")]
    public void A_realm_auction_prices_per_unit_from_its_buyout()
    {
        var listings = Found(Auctions.ParseAuctions(B("""{"auctions":[{"id":1,"item":{"id":6513},"quantity":4,"buyout":4000,"time_left":"LONG"}]}""")));
        Assert.Equal(1000, listings[0].UnitPrice);
        Assert.Equal(4, listings[0].Quantity);
    }

    [Fact(DisplayName = "a_bid_only_auction_is_skipped_rather_than_priced")]
    public void A_bid_only_auction_is_skipped_rather_than_priced()
    {
        Assert.IsType<Outcome<List<Listing>>.Empty>(Auctions.ParseAuctions(B("""{"auctions":[{"id":1,"item":{"id":6513},"quantity":1,"bid":300,"time_left":"LONG"}]}""")));
    }

    [Fact(DisplayName = "a_variant_fingerprint_is_stable_whatever_order_blizzard_sends")]
    public void A_variant_fingerprint_is_stable_whatever_order_blizzard_sends()
    {
        var a = Found(Auctions.ParseAuctions(B("""{"auctions":[{"item":{"id":1,"bonus_lists":[4279,1532],"modifiers":[{"type":28,"value":1},{"type":9,"value":70}]},"quantity":1,"buyout":100}]}""")));
        var b = Found(Auctions.ParseAuctions(B("""{"auctions":[{"item":{"id":1,"bonus_lists":[1532,4279],"modifiers":[{"type":9,"value":70},{"type":28,"value":1}]},"quantity":1,"buyout":100}]}""")));
        Assert.Equal(a[0].Variant, b[0].Variant);
        Assert.Equal("b1532,b4279,m9:70,m28:1", a[0].Variant);
    }

    [Fact(DisplayName = "a_pet_keeps_its_species_or_every_pet_prices_as_one_thing")]
    public void A_pet_keeps_its_species_or_every_pet_prices_as_one_thing()
    {
        var listings = Found(Auctions.ParseAuctions(B("""{"auctions":[{"item":{"id":82800,"pet_species_id":1442,"pet_level":1,"pet_quality_id":3},"quantity":1,"buyout":50000}]}""")));
        Assert.Equal(1442, listings[0].PetSpecies);
        Assert.Equal("pet1442:3", listings[0].Series());
        Assert.Equal((1442L, 3L), Auctions.PetSeries("pet1442:3"));
        Assert.Null(Auctions.PetSeries("b1532"));
    }

    [Fact(DisplayName = "the_cheapest_listing_is_the_price_and_the_quantities_add_up")]
    public void The_cheapest_listing_is_the_price_and_the_quantities_add_up()
    {
        var book = Auctions.DepthOf([Listed(1, 900, 5), Listed(1, 100, 2), Listed(1, 5000, 1, "b1532")]);
        Assert.Equal(2, book.Count);
        Assert.Equal(100, book[0].Cheapest);
        Assert.Equal(7, book[0].Quantity);
        Assert.Equal("b1532", book[1].Variant);
        Assert.Equal(5000, book[1].Cheapest);
    }

    [Fact(DisplayName = "the_shape_of_the_book_survives_the_snapshot")]
    public void The_shape_of_the_book_survives_the_snapshot()
    {
        var book = Auctions.DepthOf([Listed(1, 900, 400), Listed(1, 100, 1), Listed(1, 950, 99)]);
        var only = Assert.Single(book);
        Assert.Equal(100, only.Cheapest);
        Assert.Equal(500, only.Quantity);
        Assert.Equal(3, only.Listings);
        Assert.Equal(900, only.Tenth);
        Assert.Equal(900, only.Median);

        var real = Auctions.DepthOf([Listed(1, 100, 500)]);
        Assert.Equal(100, real[0].Cheapest);
        Assert.Equal(100, real[0].Tenth);
        Assert.Equal(100, real[0].Median);
    }

    [Fact(DisplayName = "the_token_price_is_copper")]
    public void The_token_price_is_copper()
    {
        Assert.Equal(2_500_000_000, Found(Auctions.ParseToken(B("""{"last_updated_timestamp":1,"price":2500000000}"""))));
    }

    [Fact(DisplayName = "auctions_are_asked_for_in_the_dynamic_namespace")]
    public void Auctions_are_asked_for_in_the_dynamic_namespace()
    {
        Assert.Contains("namespace=dynamic-us", Auctions.AuctionsOf(Region.Us, 61).Url, StringComparison.Ordinal);
        Assert.Contains("namespace=dynamic-us", Auctions.Commodities(Region.Us).Url, StringComparison.Ordinal);
    }
}
