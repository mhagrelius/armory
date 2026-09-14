using Armory.Chronicle;
using Armory.Client.Shell;
using Armory.Roster;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from the tests in <c>src/ui/chronicle_page.rs</c>, <c>src/ui/run_page.rs</c> and <c>src/ui/character_page.rs</c>: the page's pure half.</summary>
public sealed class ChronicleCardsTests
{
    private static readonly DateTimeOffset Evening = new(2026, 8, 3, 19, 0, 0, TimeSpan.Zero);

    private static Session ASession(int daysAgo, int minutes, DateOnly today) => new()
    {
        Character = new CharacterKey("emerald-dream", "Somechar"),
        DisplayName = "Somechar",
        StartedAt = new DateTimeOffset(today.AddDays(-daysAgo).ToDateTime(new TimeOnly(20, 0)), TimeSpan.Zero),
        EndedAt = new DateTimeOffset(today.AddDays(-daysAgo).ToDateTime(new TimeOnly(20, 0)), TimeSpan.Zero).AddMinutes(minutes),
    };

    private static Digest ADigest() => new Session
    {
        Character = new CharacterKey("emerald-dream", "Somechar"),
        DisplayName = "Somechar",
        RealmName = "Emerald Dream",
        Class = "Druid",
        Race = "Tauren",
        Faction = Faction.Horde,
        StartedAt = Evening,
        EndedAt = Evening.AddHours(2),
        StartLevel = 70,
        EndLevel = 70,
        StartItemLevel = 600,
        EndItemLevel = 600,
        Moments =
        [
            new Moment { At = 0, What = new Happening.Arrived("Nagrand", null, null) },
            new Moment { At = 10, What = new Happening.Completed(1, "Hero of the Mag'har", null) },
        ],
    }.Digest();

    private static Entry AnEntry() => new()
    {
        Session = ADigest().Id,
        Title = "Halaa Again",
        Body = "The wind came off the plains all evening.",
        Model = "claude-opus-5",
        WrittenAt = new DateTimeOffset(2026, 8, 4, 9, 0, 0, TimeSpan.Zero),
    };

    [Fact(DisplayName = "a_search_reaches_the_prose_and_the_log_alike")]
    public void A_search_reaches_the_prose_and_the_log_alike()
    {
        var digest = ADigest();
        var entry = AnEntry();
        Assert.True(Cards.Matches(digest, entry, "wind"));
        Assert.True(Cards.Matches(digest, entry, "halaa"));
        Assert.True(Cards.Matches(digest, null, "nagrand"));
        Assert.True(Cards.Matches(digest, null, "mag'har"));
        Assert.True(Cards.Matches(digest, null, "somechar"));
        Assert.False(Cards.Matches(digest, null, "wind"));
        Assert.False(Cards.Matches(digest, entry, "orgrimmar"));
    }

    [Fact(DisplayName = "a_tally_falls_back_to_the_headline_when_there_is_nothing_to_count")]
    public void A_tally_falls_back_to_the_headline_when_there_is_nothing_to_count()
    {
        var quiet = ADigest() with { Quests = [] };
        Assert.Equal(quiet.Headline(), Cards.Tally(quiet));
        Assert.Equal("1 quest", Cards.Tally(ADigest()));
    }

    [Fact(DisplayName = "the_ledger_bar_is_the_share_of_everything_that_moved")]
    public void The_ledger_bar_is_the_share_of_everything_that_moved()
    {
        var (earned, spent) = Cards.LedgerShares([(Purpose.Quest, 300), (Purpose.Loot, 100)], [(Purpose.Repair, 100)]);
        Assert.Equal(2, earned.Count);
        Assert.Single(spent);
        // The first segment of each book is the full-strength colour.
        Assert.True(earned[0].First && !earned[1].First && spent[0].First);
        Assert.Equal(0.8, earned.Sum(segment => segment.Share), 9);
        Assert.Equal(0.2, spent[0].Share, 9);
    }

    [Fact(DisplayName = "an_evening_that_moved_no_money_draws_no_bar")]
    public void An_evening_that_moved_no_money_draws_no_bar()
    {
        var (income, spending) = Cards.LedgerShares([], []);
        Assert.Empty(income);
        Assert.Empty(spending);
    }

    [Fact(DisplayName = "a_meta_line_says_what_the_evening_was")]
    public void A_meta_line_says_what_the_evening_was()
    {
        Assert.Equal("19:00 — 2h 0m · 1 quest", Cards.MetaLine(ADigest()));
    }

    [Fact(DisplayName = "a_day_nobody_played_is_an_absence_and_not_a_zero")]
    public void A_day_nobody_played_is_an_absence_and_not_a_zero()
    {
        var today = new DateOnly(2026, 9, 14);
        var strip = Cards.Fortnight([ASession(0, 120, today), ASession(2, 60, today)], today);
        Assert.Equal(Cards.Days, strip.Count);
        Assert.Equal(1.0, strip[Cards.Days - 1]);
        Assert.Null(strip[Cards.Days - 2]);
        Assert.Equal(0.5, strip[Cards.Days - 3]);
    }

    [Fact(DisplayName = "an_evening_older_than_the_fortnight_is_not_in_it")]
    public void An_evening_older_than_the_fortnight_is_not_in_it()
    {
        var today = new DateOnly(2026, 9, 14);
        Assert.All(Cards.Fortnight([ASession(40, 300, today)], today), bar => Assert.Null(bar));
    }

    [Fact(DisplayName = "two_evenings_on_one_day_are_one_bar")]
    public void Two_evenings_on_one_day_are_one_bar()
    {
        var today = new DateOnly(2026, 9, 14);
        var strip = Cards.Fortnight([ASession(0, 60, today), ASession(0, 60, today), ASession(1, 60, today)], today);
        Assert.Equal(1.0, strip[Cards.Days - 1]);
        Assert.Equal(0.5, strip[Cards.Days - 2]);
    }

    [Fact(DisplayName = "a fight's length says its unit")]
    public void A_fights_length_says_its_unit()
    {
        Assert.Equal("10s", Cards.FightLength(10));
        Assert.Equal("11:24", Cards.FightLength(684));
        Assert.Equal("1:05", Cards.FightLength(65));
    }

    [Fact(DisplayName = "the road names where the evening was spent, not where it ended")]
    public void The_road_names_where_the_evening_was_spent()
    {
        var digest = new Session
        {
            Character = new CharacterKey("emerald-dream", "Somechar"),
            DisplayName = "Somechar",
            StartedAt = Evening,
            EndedAt = Evening.AddHours(3),
            Moments =
            [
                new Moment { At = 0, What = new Happening.Arrived("Nagrand", null, null) },
                new Moment { At = 10_000, What = new Happening.Arrived("Dornogal", null, null) },
            ],
        }.Digest();
        var (title, detail) = Cards.RoadLine(digest);
        Assert.Equal("An evening in Nagrand", title);
        Assert.Equal("Somechar · 3h 0m", detail);
    }

    [Fact(DisplayName = "an hour is said the way a person says it")]
    public void An_hour_is_said_the_way_a_person_says_it()
    {
        Assert.Equal("midnight", Cards.Clock(0));
        Assert.Equal("7am", Cards.Clock(7));
        Assert.Equal("midday", Cards.Clock(12));
        Assert.Equal("7pm", Cards.Clock(19));
        Assert.Equal("Tuesdays, mostly, and never before 7pm.", Cards.WeekdaySentence(1, 19));
        Assert.Equal("Sundays, mostly.", Cards.WeekdaySentence(6, null));
    }

    [Fact(DisplayName = "a character never watched says so rather than counting nothing")]
    public void A_character_never_watched_says_so()
    {
        var (span, detail) = Cards.WatchedFor([], Evening);
        Assert.Equal("Not yet watched", span);
        Assert.Contains("No evening", detail, StringComparison.Ordinal);
        var (watched, _) = Cards.WatchedFor([ADigest()], Evening.AddDays(70));
        Assert.Equal("2 months recorded", watched);
    }
}
