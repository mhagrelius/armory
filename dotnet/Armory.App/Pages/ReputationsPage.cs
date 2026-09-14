using System.Globalization;
using Armory.Blizzard;
using Armory.Client.Factions;
using Armory.Client.Shell;
using Armory.Provenance;
using Armory.Roster;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Armory.App.Pages;

/// <summary>
/// Where each character stands with everybody, and who actually earned it.
/// One bar with two readings: pale is where the account already stands,
/// accent is what the character in front of you was watched earning, login
/// to logout. Only the accent moves, and only the accent is ever claimed by
/// the run. An inherited standing is never drawn in the accent, and a
/// standing nobody watched gets no bar at all: a floor drawn as a bar reads
/// as a measurement.
/// </summary>
public sealed partial class ReputationsPage : Page, ISearchable
{
    /// <summary>How many standings one group draws before it stops. The ordering puts the ones somebody has actually worked on above the cap.</summary>
    private const int Shown = 60;

    private readonly Account account;
    private readonly StackPanel column = new() { Spacing = 16 };
    private readonly StackPanel groups = new() { Spacing = 16 };
    private readonly Grid totals = new() { ColumnSpacing = 16 };
    private readonly InfoBar inheritedNote;
    private string needle = "";
    private bool byFaction;
    private bool showInherited = true;

    public ReputationsPage(Account account)
    {
        this.account = account;

        var selector = new SelectorBar();
        var byCharacter = new SelectorBarItem { Text = "By character", IsSelected = true };
        var byFactionItem = new SelectorBarItem { Text = "By faction" };
        selector.Items.Add(byCharacter);
        selector.Items.Add(byFactionItem);
        selector.SelectionChanged += (sender, _) =>
        {
            // Only on a real change, and never inline: a selection event can
            // fire during the first measure pass, and rebuilding the tree
            // there takes the process down.
            var wanted = sender.SelectedItem == byFactionItem;
            if (wanted == byFaction)
            {
                return;
            }
            byFaction = wanted;
            DispatcherQueue.TryEnqueue(Redraw);
        };
        var inherited = new ToggleSwitch { Header = null, OnContent = "Show inherited", OffContent = "Show inherited", IsOn = true, MinWidth = 0 };
        inherited.Toggled += (_, _) =>
        {
            if (inherited.IsOn == showInherited)
            {
                return;
            }
            showInherited = inherited.IsOn;
            DispatcherQueue.TryEnqueue(Redraw);
        };
        var controls = new Grid { ColumnSpacing = 16 };
        controls.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        controls.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        controls.Children.Add(selector);
        Grid.SetColumn(inherited, 1);
        controls.Children.Add(inherited);

        inheritedNote = Widgets.Standing(
            "Inherited standings are shown",
            "Where the account already stands is not what the run earned — the greyed bars came from characters nobody enrolled.");

        for (var i = 0; i < 3; i++)
        {
            totals.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        }

        var sync = Widgets.Accent("Sync", "");
        sync.Click += async (_, _) => await account.Sync();
        column.Children.Add(Widgets.Header("Reputations", "What each character actually earned, watched login to logout", sync));
        column.Children.Add(controls);
        column.Children.Add(inheritedNote);
        column.Children.Add(totals);
        column.Children.Add(groups);
        column.Children.Add(new TextBlock
        {
            Text = "Without the addon there is no observation, and the answer is zero rather than the account's standing. Falling back would be the inflation the rule exists to prevent.",
            Style = (Style)Application.Current.Resources["Caption"],
            TextWrapping = TextWrapping.Wrap,
        });

        Content = new ScrollViewer { Content = column, Padding = (Thickness)Application.Current.Resources["PagePadding"] };
        account.Changed += () => _ = DispatcherQueue.TryEnqueue(Redraw);
        Redraw();
    }

    public string SearchPlaceholder => "Search factions and characters";

    public void Search(string text)
    {
        needle = text.Trim();
        Redraw();
    }

    private void Redraw()
    {
        var roster = account.Roster;
        var standings = account.Reputations;
        var earned = account.Inputs.Provenance;
        inheritedNote.IsOpen = showInherited && standings.Values.Any(list => list.Any(standing => standing.Inherited));
        DrawTotals(roster, standings, earned);

        groups.Children.Clear();
        if (standings.Count == 0)
        {
            groups.Children.Add(Widgets.Standing(
                "No standings yet",
                "Reputations are fetched per enrolled character. Enrol somebody on Roster and sync — Blizzard has no account-wide reputation endpoint, so this is one call each."));
            return;
        }

        var drawn = byFaction ? ByFaction(roster, standings, earned) : ByCharacter(roster, standings, earned);
        if (drawn == 0)
        {
            groups.Children.Add(Widgets.Standing(
                "No matches",
                showInherited ? "Nothing here matches that." : "Nothing here matches that — and inherited standings are hidden."));
        }
    }

    /// <summary>The three claims across the whole cohort. Counted over everything the account has, not over what the search left on screen.</summary>
    private void DrawTotals(Armory.Roster.Roster roster, Dictionary<CharacterKey, List<FactionStanding>> standings, Earnings earned)
    {
        var counts = (Earned: 0, Inherited: 0, Unclear: 0);
        foreach (var character in roster.Characters)
        {
            if (!standings.TryGetValue(character.Key, out var list))
            {
                continue;
            }
            var one = Readings.Tally(list, earned.GetValueOrDefault(character.Key));
            counts = (counts.Earned + one.Earned, counts.Inherited + one.Inherited, counts.Unclear + one.Unclear);
        }
        totals.Children.Clear();
        // Only the first is the accent. The other two are real numbers about
        // the account and neither is work this run may claim.
        var earnedCard = Widgets.Stat("Earned in this run", N(counts.Earned), "watched login to logout");
        var inheritedCard = Widgets.Stat("Inherited", N(counts.Inherited), "at the ceiling before the run began");
        var unclearCard = Widgets.Stat("Cannot tell", N(counts.Unclear), "nobody was watching");
        Grid.SetColumn(inheritedCard, 1);
        Grid.SetColumn(unclearCard, 2);
        totals.Children.Add(earnedCard);
        totals.Children.Add(inheritedCard);
        totals.Children.Add(unclearCard);
    }

    private bool Matches(FactionStanding standing, Character character) =>
        needle.Length == 0
        || standing.Name.Contains(needle, StringComparison.OrdinalIgnoreCase)
        || character.DisplayName.Contains(needle, StringComparison.OrdinalIgnoreCase);

    /// <summary>The standings the search and the inherited toggle have left, work first and furthest first, so the cap falls on the factions nobody has touched.</summary>
    private static List<FactionStanding> Ordered(IEnumerable<FactionStanding> standings, Earned? earned) =>
        standings
            .OrderByDescending(standing => earned is not null && earned.HasTouched(standing.Faction) ? Readings.Share(standing, earned.With(standing.Faction)) : -1.0)
            .ThenBy(standing => standing.Name, StringComparer.OrdinalIgnoreCase)
            .ToList();

    /// <summary>A group per character: who they are, then what they stand at.</summary>
    private int ByCharacter(Armory.Roster.Roster roster, Dictionary<CharacterKey, List<FactionStanding>> standings, Earnings earnings)
    {
        var drawn = 0;
        var light = Widgets.IsLight(this);
        foreach (var character in roster.Characters)
        {
            if (!standings.TryGetValue(character.Key, out var list))
            {
                continue;
            }
            var earned = earnings.GetValueOrDefault(character.Key);
            var matching = Ordered(list.Where(standing => showInherited || !standing.Inherited).Where(standing => Matches(standing, character)), earned);
            if (matching.Count == 0)
            {
                continue;
            }

            var group = new StackPanel { Spacing = 8 };
            group.Children.Add(Strip(character, list, earned, light));

            // The ones nothing can answer are gathered into a single card
            // rather than drawn one apiece: on an account with no collector
            // addon every standing is unclear, and a card each is two hundred
            // identical grey panels repeating one sentence.
            var unclear = matching.Where(standing => Readings.Of(standing, earned) == Reading.Unclear).ToList();
            var measured = matching.Where(standing => Readings.Of(standing, earned) != Reading.Unclear).ToList();
            var table = new StackPanel();
            table.Children.Add(TableHead());
            foreach (var standing in measured.Take(Shown))
            {
                table.Children.Add(Row(standing.Name, character, standing, earned, light));
                drawn++;
            }
            if (measured.Count > Shown)
            {
                table.Children.Add(Widgets.Text(string.Create(CultureInfo.InvariantCulture, $"and {measured.Count - Shown} more — search to reach them."), "Caption"));
            }
            if (measured.Count > 0)
            {
                group.Children.Add(Widgets.Card(table));
            }
            if (unclear.Count > 0)
            {
                drawn += unclear.Count;
                group.Children.Add(UnclearCard(unclear.Select(standing => standing.Name).ToList()));
            }
            groups.Children.Add(group);
        }
        return drawn;
    }

    /// <summary>A group per faction, so a whole roster can be compared down a column. Furthest first, on what each was watched earning.</summary>
    private int ByFaction(Armory.Roster.Roster roster, Dictionary<CharacterKey, List<FactionStanding>> standings, Earnings earnings)
    {
        var byFactionId = new Dictionary<long, (string Name, List<(Character Who, FactionStanding Standing, Earned? Earned)> Members)>();
        foreach (var character in roster.Characters)
        {
            if (!standings.TryGetValue(character.Key, out var list))
            {
                continue;
            }
            foreach (var standing in list)
            {
                if (!showInherited && standing.Inherited)
                {
                    continue;
                }
                if (!Matches(standing, character))
                {
                    continue;
                }
                if (!byFactionId.TryGetValue(standing.Faction, out var entry))
                {
                    entry = (standing.Name, []);
                    byFactionId[standing.Faction] = entry;
                }
                entry.Members.Add((character, standing, earnings.GetValueOrDefault(character.Key)));
            }
        }

        var drawn = 0;
        var light = Widgets.IsLight(this);
        foreach (var (name, members) in byFactionId.Values.OrderBy(entry => entry.Name, StringComparer.OrdinalIgnoreCase))
        {
            var ordered = members
                .OrderByDescending(member => member.Earned is { } earned ? Readings.Share(member.Standing, earned.With(member.Standing.Faction)) : 0.0)
                .ThenByDescending(member => member.Standing.Renown)
                .ThenBy(member => member.Who.DisplayName, StringComparer.OrdinalIgnoreCase)
                .ToList();
            var group = new StackPanel { Spacing = 8 };
            group.Children.Add(new TextBlock { Text = name.ToUpperInvariant(), Style = (Style)Application.Current.Resources["Caption"] });
            var unclear = ordered.Where(member => Readings.Of(member.Standing, member.Earned) == Reading.Unclear).ToList();
            var measured = ordered.Where(member => Readings.Of(member.Standing, member.Earned) != Reading.Unclear).ToList();
            if (measured.Count > 0)
            {
                var table = new StackPanel();
                table.Children.Add(TableHead("Character"));
                foreach (var (who, standing, earned) in measured)
                {
                    table.Children.Add(Row(who.DisplayName, who, standing, earned, light));
                    drawn++;
                }
                group.Children.Add(Widgets.Card(table));
            }
            if (unclear.Count > 0)
            {
                drawn += unclear.Count;
                group.Children.Add(UnclearCard(unclear.Select(member => member.Who.DisplayName).ToList()));
            }
            groups.Children.Add(group);
        }
        return drawn;
    }

    // -- the pieces ----------------------------------------------------------------

    /// <summary>Who this group is about, and how their standings divide up.</summary>
    private static Border Strip(Character character, List<FactionStanding> standings, Earned? earned, bool light)
    {
        var line = new Grid { ColumnSpacing = 12 };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var text = new StackPanel { Spacing = 1 };
        text.Children.Add(new TextBlock { Text = character.DisplayName, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], Foreground = Widgets.ClassBrush(character.Class, light) });
        // Every character with standings is an enrolled one: reputations are
        // one call each and are only ever fetched for the cohort.
        text.Children.Add(new TextBlock { Text = $"{character.RealmName} · enrolled", Style = (Style)Application.Current.Resources["Caption"] });
        line.Children.Add(text);
        var (earnedCount, inherited, unclear) = Readings.Tally(standings, earned);
        var parts = new List<string> { string.Create(CultureInfo.InvariantCulture, $"{earnedCount} EARNED"), string.Create(CultureInfo.InvariantCulture, $"{inherited} INHERITED") };
        if (unclear > 0)
        {
            parts.Add(string.Create(CultureInfo.InvariantCulture, $"{unclear} CANNOT TELL"));
        }
        var counts = new TextBlock { Text = string.Join(" · ", parts), Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(counts, 1);
        line.Children.Add(counts);
        return Widgets.Card(line, dense: true);
    }

    private static Grid Columns()
    {
        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(150) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(170) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(130) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(120) });
        return grid;
    }

    private static Grid TableHead(string first = "Faction")
    {
        var head = Columns();
        head.Margin = new Thickness(0, 0, 0, 6);
        var labels = new[] { first, "Standing", "Progress", "Earned by", "Where it came from" };
        for (var i = 0; i < labels.Length; i++)
        {
            var label = new TextBlock { Text = labels[i], Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = i == 4 ? HorizontalAlignment.Right : HorizontalAlignment.Left };
            Grid.SetColumn(label, i);
            head.Children.Add(label);
        }
        return head;
    }

    /// <summary>One standing, and what this character personally did for it.</summary>
    private static Border Row(string title, Character who, FactionStanding standing, Earned? earned, bool light)
    {
        var mine = earned?.With(standing.Faction) ?? new EarnedReputation();
        var reading = Readings.Of(standing, earned);
        var grid = Columns();

        var name = new StackPanel { Spacing = 1, VerticalAlignment = VerticalAlignment.Center };
        name.Children.Add(new TextBlock { Text = title, TextWrapping = TextWrapping.Wrap });
        name.Children.Add(new TextBlock
        {
            Text = reading switch
            {
                Reading.Inherited => $"{Readings.Tier(standing)} before this run began, earned by a character nobody enrolled. It cannot move — attest it or leave it out.",
                _ => Readings.EarnedLine(standing, mine, who.DisplayName),
            },
            Style = (Style)Application.Current.Resources["Caption"],
            TextWrapping = TextWrapping.Wrap,
        });
        grid.Children.Add(name);

        var tier = new TextBlock { Text = Readings.Tier(standing), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], VerticalAlignment = VerticalAlignment.Center };
        Grid.SetColumn(tier, 1);
        grid.Children.Add(tier);

        // The pale reading is always the whole of the account's standing;
        // only the accent moves. Nothing at all for an inherited standing.
        var fill = reading == Reading.Inherited ? 1.0 : Readings.Share(standing, mine);
        var bar = Widgets.Bar(fill, inherited: reading == Reading.Inherited);
        bar.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(bar, 2);
        grid.Children.Add(bar);

        var by = reading == Reading.Earned
            ? Widgets.Named(who, who.DisplayName, light)
            : new StackPanel { Children = { new TextBlock { Text = "—", Style = (Style)Application.Current.Resources["Caption"] } } };
        by.VerticalAlignment = VerticalAlignment.Center;
        Grid.SetColumn(by, 3);
        grid.Children.Add(by);

        var origin = new TextBlock
        {
            Text = reading switch
            {
                Reading.Earned => Readings.Badge(reading, mine),
                Reading.Inherited => "Inherited",
                _ => "Nothing earned yet",
            },
            Style = (Style)Application.Current.Resources["Caption"],
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
        };
        if (reading == Reading.Earned)
        {
            origin.Foreground = Widgets.AccentBrush;
        }
        Grid.SetColumn(origin, 4);
        grid.Children.Add(origin);

        return new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = grid };
    }

    /// <summary>Everything the page cannot answer, in one card. Never hidden and never counted as nothing: the size of this list is half of what makes the rest of the page believable.</summary>
    private static Border UnclearCard(List<string> names)
    {
        var card = new StackPanel { Spacing = 8 };
        card.Children.Add(Widgets.CardHead(names.Count == 1 ? "1 standing" : string.Create(CultureInfo.InvariantCulture, $"{names.Count} standings"), "CANNOT TELL"));
        card.Children.Add(Widgets.Text("The client does not maintain a total earned for these, so Armory will not guess which character did the work. Install the collector addon and it will watch them from the next login onwards.", "Caption"));
        // The factions themselves, named. A count with nothing under it
        // invites the question this card exists to answer.
        card.Children.Add(new TextBlock { Text = string.Join(" · ", names), TextWrapping = TextWrapping.Wrap, Foreground = Widgets.Tertiary });
        var border = Widgets.Card(card);
        border.Opacity = 0.75;
        return border;
    }

    private static string N(int n) => n.ToString(CultureInfo.InvariantCulture);
}
