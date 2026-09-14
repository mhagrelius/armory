using Armory.Client.Sharing;
using Armory.Sharing;
using Xunit;

namespace Armory.Tests.Client;

/// <summary>Ported from the tests in <c>src/ui/sync_dialog.rs</c>.</summary>
public sealed class SharingWordingTests
{
    private static SharingStatus Sharing() => new() { Server = "nas:8084" };

    private static List<(string Name, long Rows)> Held(params string[] names) =>
        names.Select(name => (name, 1L)).ToList();

    [Fact(DisplayName = "every_table_that_travels_has_a_name_somebody_would_say")]
    public void Every_table_that_travels_has_a_name_somebody_would_say()
    {
        // A new table in `sync::TABLES` with no entry here shows up in the
        // dialog as `earned_reputation`, which is the failure this catches.
        foreach (var table in Tables.All)
        {
            Assert.NotEqual(table.Name, SharingWording.Pretty(table.Name));
        }
    }

    [Fact(DisplayName = "a_pass_that_did_nothing_says_so_rather_than_listing_three_zeroes")]
    public void A_pass_that_did_nothing_says_so_rather_than_listing_three_zeroes()
    {
        var state = Sharing() with { Last = new PassSummary("just now", 0, 0, 0, 0, null) };
        Assert.Equal("just now — nothing to do.", SharingWording.Describe(state));
    }

    [Fact(DisplayName = "a_pass_that_moved_things_names_which_way_they_went")]
    public void A_pass_that_moved_things_names_which_way_they_went()
    {
        var state = Sharing() with { Last = new PassSummary("just now", 12, 3, 1, 0, null) };
        Assert.Equal("just now — 12 up, 3 down, 1 removed.", SharingWording.Describe(state));
    }

    [Fact(DisplayName = "rows_the_other_end_could_not_read_are_said_out_loud")]
    public void Rows_the_other_end_could_not_read_are_said_out_loud()
    {
        var state = Sharing() with { Last = new PassSummary("just now", 0, 4, 0, 2, null) };
        Assert.Contains("2 not understood", SharingWording.Describe(state), StringComparison.Ordinal);
    }

    [Fact(DisplayName = "a_failure_carries_how_many_in_a_row_because_one_is_not_news")]
    public void A_failure_carries_how_many_in_a_row_because_one_is_not_news()
    {
        var state = Sharing() with
        {
            Failures = 4,
            Last = new PassSummary("an hour ago", 0, 0, 0, 0, "could not reach nas:8084"),
        };
        var said = SharingWording.Describe(state);
        Assert.Contains("failed 4 times in a row", said, StringComparison.Ordinal);
        Assert.Contains("could not reach", said, StringComparison.Ordinal);
    }

    [Fact(DisplayName = "with_no_server_it_says_sharing_is_off_rather_than_never")]
    public void With_no_server_it_says_sharing_is_off_rather_than_never()
    {
        Assert.Equal("Sharing is off.", SharingWording.Describe(new SharingStatus()));
    }

    [Fact(DisplayName = "a_server_that_has_not_answered_still_offers_default")]
    public void A_server_that_has_not_answered_still_offers_default()
    {
        Assert.Equal(["default"], SharingWording.Options(null, ""));
        Assert.Equal(0, SharingWording.Selected(SharingWording.Options(null, ""), ""));
    }

    /// <summary>
    /// The case that would otherwise move a machine somewhere else: the server
    /// has been emptied, so it holds nothing this machine has heard of, and
    /// the list it answers with does not contain the account this machine is on.
    /// </summary>
    [Fact(DisplayName = "the_account_this_machine_is_on_is_offered_even_when_the_server_lost_it")]
    public void The_account_this_machine_is_on_is_offered_even_when_the_server_lost_it()
    {
        var chosen = SharingWording.Options(Held("someone-else"), "PLAYER1");
        Assert.Contains("PLAYER1", chosen);
        Assert.Equal(chosen.IndexOf("PLAYER1"), SharingWording.Selected(chosen, "PLAYER1"));
    }

    [Fact(DisplayName = "an_account_the_server_holds_is_not_offered_twice")]
    public void An_account_the_server_holds_is_not_offered_twice()
    {
        var chosen = SharingWording.Options(Held("default", "PLAYER1"), "PLAYER1");
        Assert.Equal(["default", "PLAYER1"], chosen);
    }

    /// <summary>
    /// The refusal list is the server's, and a name that fails here is a 400
    /// on a pass an hour later if it is allowed through.
    /// </summary>
    [Fact(DisplayName = "a_name_the_server_would_refuse_is_not_a_name")]
    public void A_name_the_server_would_refuse_is_not_a_name()
    {
        foreach (var bad in new[] { "", ".", "..", "...", "a/b", "../etc", "a b", "réalto", "a#1" })
        {
            Assert.False(SharingWording.ValidAccount(bad), $"{bad} should be refused");
        }
        Assert.False(SharingWording.ValidAccount(new string('a', SharingWording.MaxAccount + 1)));
        foreach (var good in new[] { "default", "PLAYER1", "second-account_2.0", "a" })
        {
            Assert.True(SharingWording.ValidAccount(good), $"{good} should be allowed");
        }
    }

    /// <summary>
    /// Battle.net folders are very often <c>12345678#1</c>, which is the whole
    /// reason this is not a straight copy of the folder name.
    /// </summary>
    [Fact(DisplayName = "a_folder_name_becomes_one_the_server_will_take")]
    public void A_folder_name_becomes_one_the_server_will_take()
    {
        Assert.Equal("PLAYER1", SharingWording.AccountFromFolder("PLAYER1"));
        Assert.Equal("12345678-1", SharingWording.AccountFromFolder("12345678#1"));
        Assert.Equal("PLAYER1", SharingWording.AccountFromFolder("  PLAYER1  "));
        // Nothing usable left, rather than a name made of dashes.
        Assert.Equal("default", SharingWording.AccountFromFolder(".."));
        Assert.Equal("default", SharingWording.AccountFromFolder(""));
        Assert.True(SharingWording.ValidAccount(SharingWording.AccountFromFolder(new string('x', 200))));
    }
}
