using System.Globalization;
using Armory.Client.Shell;
using Armory.Zones;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Armory.App.Pages;

/// <summary>
/// A page a place, joined on <c>UiMapID</c>: what it is (the corpus, compiled
/// in), what happened in its dungeons (the Adventure Guide, with Armory's own
/// words standing in where the guide is blank), and what <i>you</i> did there
/// (the evenings, from the collector addon). The index is a table, visited
/// zones first; a row opens the place. The port of <c>ui/zone_page.rs</c>
/// against the handoff's Zones screen.
/// </summary>
public sealed partial class ZonesPage : Page, ISearchable
{
    private const int SessionsShown = 60;
    private const int BossesShown = 6;
    private const int VisitsShown = 8;
    private const int ChipsShown = 5;
    private const int KillersShown = 6;
    private const int SpoilsShown = 8;

    private readonly Account account;
    private readonly StackPanel column = new() { Spacing = 16 };
    private readonly ScrollViewer scroller;
    private string needle = "";
    private List<Place> places = [];
    private Place? open;

    public ZonesPage(Account account)
    {
        this.account = account;
        scroller = new ScrollViewer { Content = column, Padding = (Thickness)Application.Current.Resources["PagePadding"] };
        Content = scroller;
        account.Changed += Reload;
        Reload();
    }

    public string SearchPlaceholder => "Search zones";

    public void Search(string text)
    {
        needle = text.Trim();
        open = null;
        Redraw();
    }

    /// <summary>
    /// Every place, assembled on each read rather than held: the corpus is
    /// compiled in and the chronicle is a few hundred sessions, and holding
    /// a second copy is how the two come to disagree after a sync.
    /// </summary>
    private async void Reload()
    {
        var assembled = await account.Store.On(store =>
        {
            var sessions = store.SessionsHeld(SessionsShown).Match(held => held, _ => []);
            var tallies = store.TalliesHeld().Match(held => held, _ => []);
            var guide = store.GuideHeld().Match(held => held, _ => new Guide());
            var items = store.Items().Match(held => held, _ => []).ToDictionary(pair => pair.Key, pair => new ItemFact(pair.Value.Name, pair.Value.Sellable));
            // The cheapest price and quantity listed, by item, across the
            // region-wide market and every watched realm. Gear carries
            // variants, so this takes the cheapest: a floor, which is the
            // honest figure when Blizzard publishes no dictionary for what
            // the variants mean.
            var market = new Dictionary<long, (long Cheapest, long Quantity)>();
            var realms = store.WatchedRealmList().Match(list => list.Select(realm => realm.RealmId).ToList(), _ => []);
            foreach (var realm in realms.Prepend(0))
            {
                foreach (var (itemId, cheapest, quantity) in store.SnapshotAll(realm).Match(rows => rows, _ => []))
                {
                    market[itemId] = market.TryGetValue(itemId, out var held)
                        ? (Math.Min(held.Cheapest, cheapest), held.Quantity + quantity)
                        : (cheapest, quantity);
                }
            }
            var written = Places.Unwritten();
            return Places.Corpus()
                .Where(lore => lore.Map is not null)
                .Select(lore => Places.Assemble(lore.Map!.Value, lore, guide, written, sessions, tallies, items, market))
                .Where(place => place.IsWorthShowing)
                // Where you have been, longest first; then everywhere else.
                .OrderByDescending(place => place.Spent)
                .ThenByDescending(place => place.Visits.Count)
                .ThenBy(place => place.Name, StringComparer.OrdinalIgnoreCase)
                .ToList();
        });
        places = assembled;
        if (open is { } was)
        {
            open = places.FirstOrDefault(place => place.Map == was.Map);
        }
        Redraw();
    }

    private void Redraw()
    {
        column.Children.Clear();
        if (open is { } place)
        {
            DrawPlace(place);
            return;
        }

        var shown = places.Where(place => needle.Length == 0 || place.Name.Contains(needle, StringComparison.OrdinalIgnoreCase)).ToList();
        var recorded = places.Sum(place => place.Spent);
        var summary = string.Create(CultureInfo.InvariantCulture, $"{places.Count} zones · {(recorded > 0 ? Tally.Counters.Spent(recorded) + " recorded" : "nothing recorded yet")}");
        column.Children.Add(Widgets.Header("Zones", "Longest first, across every character", Widgets.Text(summary, "Caption")));
        column.Children.Add(Widgets.Standing("From the collector addon", "Nothing in any API records time spent in a zone, so this table exists only for evenings the addon watched."));

        if (shown.Count == 0)
        {
            column.Children.Add(Widgets.Card(Widgets.Text(needle.Length == 0
                ? "Armory ships a history for a hundred and forty-three places. Install the collector addon and play, and the ones you have been to rise to the top with your own evenings beside them."
                : "Nothing here matches that.", "Secondary")));
            return;
        }

        var been = shown.Where(place => place.Spent > 0).ToList();
        if (been.Count > 0)
        {
            column.Children.Add(Widgets.Card(Table("Where you have been", "Longest first, across every character", been)));
        }
        var rest = shown.Where(place => place.Spent == 0).ToList();
        if (rest.Count > 0)
        {
            column.Children.Add(Widgets.Card(Table("Everywhere else", "Nothing recorded here yet — the history is worth reading anyway", rest)));
        }
    }

    // -- the index ------------------------------------------------------------

    private static Grid Columns()
    {
        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(110) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(72) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(72) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(190) });
        return grid;
    }

    private StackPanel Table(string title, string note, List<Place> rows)
    {
        var table = new StackPanel();
        table.Children.Add(Widgets.CardHead(title, note));
        var head = Columns();
        head.Margin = new Thickness(0, 0, 0, 6);
        var labels = new[] { "Zone", "Time", "Quests", "Deaths", "Rares" };
        for (var i = 0; i < labels.Length; i++)
        {
            var label = new TextBlock { Text = labels[i], Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = i is 2 or 3 ? HorizontalAlignment.Right : HorizontalAlignment.Left };
            Grid.SetColumn(label, i);
            head.Children.Add(label);
        }
        table.Children.Add(head);
        foreach (var place in rows)
        {
            table.Children.Add(Row(place));
        }
        return table;
    }

    private Border Row(Place place)
    {
        var grid = Columns();
        var name = new StackPanel();
        var title = new TextBlock { Text = place.Name, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] };
        name.Children.Add(title);
        var parts = new List<string>();
        if (place.Lore is { Expansion.Length: > 0 } lore)
        {
            parts.Add(lore.Expansion);
        }
        if (place.Visits.Count > 0)
        {
            parts.Add(Chronicle.Prose.Plural(place.Visits.Count, "evening", "evenings"));
        }
        if (place.Delves.Count > 0)
        {
            parts.Add(Chronicle.Prose.Plural(place.Delves.Count, "dungeon", "dungeons"));
        }
        if (parts.Count > 0)
        {
            name.Children.Add(new TextBlock { Text = string.Join(" · ", parts), Style = (Style)Application.Current.Resources["Caption"] });
        }
        grid.Children.Add(name);

        // The hours are the account's own work, so they are the one accent
        // thing on the row. A zone nobody has been to carries no figure.
        var time = new TextBlock { Text = place.Spent > 0 ? ZoneProse.Span(place.Spent) : "—", VerticalAlignment = VerticalAlignment.Center, FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Cascadia Mono, Consolas") };
        if (place.Spent > 0)
        {
            time.Foreground = Widgets.AccentBrush;
        }
        else
        {
            time.Foreground = Widgets.Tertiary;
        }
        Grid.SetColumn(time, 1);
        grid.Children.Add(time);

        var quests = place.Visits.Sum(visit => visit.Quests.Count);
        var deaths = place.Visits.Sum(visit => visit.Deaths.Count);
        var questCell = Figure(quests);
        Grid.SetColumn(questCell, 2);
        grid.Children.Add(questCell);
        var deathCell = Figure(deaths);
        Grid.SetColumn(deathCell, 3);
        grid.Children.Add(deathCell);

        var rares = ZoneProse.Tallied(place.Visits.SelectMany(visit => visit.Rares)).Select(entry => entry.Name).ToList();
        var rareCell = new TextBlock { Text = rares.Count == 0 ? "—" : string.Join(", ", rares), VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis, Style = (Style)Application.Current.Resources["Caption"] };
        if (rares.Count == 0)
        {
            rareCell.Foreground = Widgets.Tertiary;
        }
        Grid.SetColumn(rareCell, 4);
        grid.Children.Add(rareCell);

        var row = new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = grid };
        row.PointerPressed += (_, _) => Open(place);
        row.PointerEntered += (_, _) => ProtectedCursor = Microsoft.UI.Input.InputSystemCursor.Create(Microsoft.UI.Input.InputSystemCursorShape.Hand);
        row.PointerExited += (_, _) => ProtectedCursor = null;
        return row;
    }

    private static TextBlock Figure(int count)
    {
        var block = new TextBlock { Text = count.ToString(CultureInfo.InvariantCulture), HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Center };
        if (count == 0)
        {
            block.Foreground = Widgets.Tertiary;
        }
        return block;
    }

    /// <summary>Open a place by name. For the preview, which has no pointer to click with.</summary>
    public void OpenNamed(string name)
    {
        if (places.FirstOrDefault(place => place.Name == name) is { } place)
        {
            Open(place);
        }
    }

    private void Open(Place place)
    {
        open = place;
        DispatcherQueue.TryEnqueue(() =>
        {
            Redraw();
            scroller.ChangeView(null, 0, null, disableAnimation: true);
        });
    }

    // -- one place ------------------------------------------------------------

    private void DrawPlace(Place place)
    {
        var back = Widgets.Standard("Zones");
        back.Content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Children = { new FontIcon { Glyph = "", FontSize = 14 }, new TextBlock { Text = "All zones" } } };
        back.Click += (_, _) =>
        {
            open = null;
            DispatcherQueue.TryEnqueue(Redraw);
        };

        // The expansion and the map id, and neither is accent: a UiMapID is Blizzard's filing, not anybody's work.
        var said = new List<string>();
        if (place.Lore is { Expansion.Length: > 0 } lore)
        {
            said.Add(lore.Expansion);
        }
        said.Add(string.Create(CultureInfo.InvariantCulture, $"UiMapID {place.Map}"));
        column.Children.Add(Widgets.Header(place.Name, string.Join(" · ", said), back));

        // The hours are the account's own, so this is the accent half of the
        // band. Each half is a number somebody earned or it is not drawn.
        var parts = new List<string>();
        if (place.Visits.Count > 0)
        {
            parts.Add(Chronicle.Prose.Plural(place.Visits.Count, "evening", "evenings"));
        }
        var deaths = place.Visits.Sum(visit => visit.Deaths.Count);
        if (deaths > 0)
        {
            parts.Add(Chronicle.Prose.Plural(deaths, "death", "deaths"));
        }
        if (place.Spent > 0 || parts.Count > 0)
        {
            var stats = new Grid { ColumnSpacing = 16 };
            stats.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            stats.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            stats.Children.Add(Widgets.Stat("Time here", place.Spent > 0 ? ZoneProse.Span(place.Spent) : "—", "Across every character"));
            var evenings = Widgets.Stat("Evenings", parts.Count > 0 ? string.Join(" · ", parts) : "—", "From the collector addon");
            Grid.SetColumn(evenings, 1);
            stats.Children.Add(evenings);
            column.Children.Add(stats);
        }

        var main = new StackPanel { Spacing = 16 };
        var rail = new StackPanel { Spacing = 16 };
        if (place.Lore is { } about)
        {
            // Prose first means prose in its own container above everything
            // else, never first inside a group.
            if (about.Summary.Length > 0)
            {
                main.Children.Add(Widgets.Card(new TextBlock { Text = about.Summary, TextWrapping = TextWrapping.Wrap, FontFamily = new Microsoft.UI.Xaml.Media.FontFamily("Georgia, Cambria, serif"), FontSize = 15, LineHeight = 24 }));
            }
            if (about.History.Length > 0)
            {
                var history = new StackPanel();
                history.Children.Add(Widgets.CardHead("History"));
                history.Children.Add(Paragraph(about.History));
                main.Children.Add(Widgets.Card(history));
            }
            if (about.Factions.Count > 0 || about.Notable.Count > 0)
            {
                main.Children.Add(Widgets.Card(WhoAndWhat(about)));
            }
        }
        if (place.Visits.Count > 0)
        {
            main.Children.Add(Widgets.Card(Evenings(place)));
        }
        foreach (var delve in place.Delves)
        {
            main.Children.Add(Widgets.Card(DelveCard(delve)));
        }

        var killers = ZoneProse.Killers(place);
        if (killers.Count > 0)
        {
            var rows = new StackPanel { Spacing = 6 };
            rows.Children.Add(Widgets.CardHead("What keeps killing you"));
            var most = Math.Max(killers[0].Count, 1);
            foreach (var (killer, count) in killers.Take(KillersShown))
            {
                var line = new Grid { ColumnSpacing = 10 };
                line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(56) });
                line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
                line.Children.Add(new TextBlock { Text = killer, Style = (Style)Application.Current.Resources["Caption"], TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center });
                var bar = new ProgressBar { Style = (Style)Application.Current.Resources["ThinBar"], Minimum = 0, Maximum = 1, Value = count / (double)most, VerticalAlignment = VerticalAlignment.Center };
                Grid.SetColumn(bar, 1);
                line.Children.Add(bar);
                var figure = new TextBlock { Text = count.ToString(CultureInfo.InvariantCulture), Style = (Style)Application.Current.Resources["Figure"] };
                Grid.SetColumn(figure, 2);
                line.Children.Add(figure);
                rows.Children.Add(line);
            }
            rail.Children.Add(Widgets.Card(rows));
        }
        if (place.Spoils.Count > 0)
        {
            var spoils = new StackPanel { Spacing = 8 };
            spoils.Children.Add(Widgets.CardHead("Worth farming here"));
            spoils.Children.Add(Widgets.Text("Bind-on-Equip only — the rest has no market at any price.", "Caption"));
            foreach (var spoil in place.Spoils.Take(SpoilsShown))
            {
                spoils.Children.Add(SpoilRow(spoil));
            }
            rail.Children.Add(Widgets.Card(spoils));
        }
        // Whenever any of the corpus's prose is on screen, so is where it
        // came from. Attribution is a condition of the licence.
        if (place.Lore is { } sourced)
        {
            var sources = new StackPanel { Spacing = 4 };
            sources.Children.Add(Widgets.CardHead("Sources"));
            foreach (var source in sourced.Sources)
            {
                sources.Children.Add(new HyperlinkButton { Content = source.Title, NavigateUri = new Uri(source.Url), Padding = new Thickness(0, 2, 0, 2) });
            }
            sources.Children.Add(Widgets.Text("Summarised in Armory's own words under CC BY-SA 4.0. Shipped with the application; nothing is fetched.", "Caption"));
            rail.Children.Add(Widgets.Card(sources));
        }

        if (rail.Children.Count == 0)
        {
            column.Children.Add(main);
            return;
        }
        var split = new Grid { ColumnSpacing = 16 };
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1.4, GridUnitType.Star) });
        split.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        split.Children.Add(main);
        Grid.SetColumn(rail, 1);
        split.Children.Add(rail);
        column.Children.Add(split);
    }

    /// <summary>Who is here, and what is worth walking to. Every faction plain: the corpus records who is here and never whether they will shoot at you.</summary>
    private static Grid WhoAndWhat(Lore lore)
    {
        var columns = new Grid { ColumnSpacing = 26 };
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(190) });
        columns.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        if (lore.Factions.Count > 0)
        {
            var who = new StackPanel { Spacing = 6 };
            who.Children.Add(Widgets.CardHead("Who is here"));
            who.Children.Add(Chips(lore.Factions.Select(faction => (faction, Tone.Plain))));
            columns.Children.Add(who);
        }
        if (lore.Notable.Count > 0)
        {
            var what = new StackPanel { Spacing = 6 };
            what.Children.Add(Widgets.CardHead("Notable"));
            foreach (var named in lore.Notable)
            {
                var line = new Grid { ColumnSpacing = 10 };
                line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(130) });
                line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                line.Children.Add(new TextBlock { Text = named.Name, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextWrapping = TextWrapping.Wrap });
                var note = Widgets.Text(named.What, "Caption");
                Grid.SetColumn(note, 1);
                line.Children.Add(note);
                what.Children.Add(line);
            }
            Grid.SetColumn(what, 1);
            columns.Children.Add(what);
        }
        return columns;
    }

    /// <summary>The evenings spent here, newest first, each with what it closed and what it cost.</summary>
    private static StackPanel Evenings(Place place)
    {
        var stack = new StackPanel { Spacing = 12 };
        stack.Children.Add(Widgets.CardHead("What you did here", "From the collector addon — nothing in any API records this"));
        foreach (var visit in place.Visits.Take(VisitsShown))
        {
            var block = new StackPanel { Spacing = 5 };
            var heading = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 9 };
            // The year is carried: an evening in a levelling zone is as
            // likely to be three years old as three days.
            heading.Children.Add(new TextBlock { Text = visit.At.ToLocalTime().ToString("d MMMM yyyy", CultureInfo.InvariantCulture), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] });
            heading.Children.Add(new TextBlock { Text = visit.Character, Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Bottom });
            block.Children.Add(heading);
            if (visit.Quests.Count > 0)
            {
                block.Children.Add(Widgets.Text(ZoneProse.TurnedIn(visit.Quests), "Caption"));
            }
            var chips = new List<(string, Tone)>();
            var deaths = ZoneProse.Tallied(visit.Deaths);
            chips.AddRange(deaths.Take(ChipsShown).Select(entry => (ZoneProse.Counted("Died to", entry.Name, entry.Count), Tone.Negative)));
            if (deaths.Count > ChipsShown)
            {
                chips.Add((string.Create(CultureInfo.InvariantCulture, $"and {deaths.Count - ChipsShown} more"), Tone.Negative));
            }
            var rares = ZoneProse.Tallied(visit.Rares);
            chips.AddRange(rares.Take(ChipsShown).Select(entry => (ZoneProse.Counted("Rare:", entry.Name, entry.Count), Tone.Accent)));
            if (rares.Count > ChipsShown)
            {
                chips.Add((string.Create(CultureInfo.InvariantCulture, $"and {rares.Count - ChipsShown} more"), Tone.Accent));
            }
            if (chips.Count > 0)
            {
                block.Children.Add(Chips(chips));
            }
            stack.Children.Add(block);
        }
        if (place.Visits.Count > VisitsShown)
        {
            stack.Children.Add(Widgets.Text(string.Create(CultureInfo.InvariantCulture, $"and {place.Visits.Count - VisitsShown} more"), "Caption"));
        }
        return stack;
    }

    /// <summary>
    /// One dungeon or raid, with whoever's words are available, and said
    /// out loud whose. What it assumes and where its sources disagree are
    /// drawn flat, never behind an expander.
    /// </summary>
    private static StackPanel DelveCard(Delve delve)
    {
        var card = new StackPanel { Spacing = 9 };
        var whose = delve.Ours ? "Armory's words — the guide is blank" : "The Adventure Guide";
        card.Children.Add(Widgets.CardHead(delve.Name, whose));
        if (delve.Description.Length > 0)
        {
            card.Children.Add(Paragraph(delve.Description));
        }
        if (delve.Bosses.Count > 0)
        {
            // Blizzard's own data carries trailing punctuation on a few.
            var chips = delve.Bosses.Take(BossesShown).Select(boss => (boss.Name.TrimEnd(',', ' '), Tone.Plain)).ToList();
            if (delve.Bosses.Count > BossesShown)
            {
                chips.Add((string.Create(CultureInfo.InvariantCulture, $"and {delve.Bosses.Count - BossesShown} more"), Tone.Plain));
            }
            card.Children.Add(Chips(chips));
        }
        if (delve.Assumes is { Length: > 0 } assumes)
        {
            var callout = new StackPanel { Spacing = 4 };
            callout.Children.Add(new TextBlock { Text = "Assumes you know", Style = (Style)Application.Current.Resources["Caption"] });
            callout.Children.Add(Paragraph(assumes));
            card.Children.Add(new Border { Style = (Style)Application.Current.Resources["Quote"], Child = callout });
        }
        foreach (var bit in delve.Disputed)
        {
            card.Children.Add(Widgets.Text($"Disputed: {bit}", "Caption"));
        }
        return card;
    }

    /// <summary>One thing worth carrying out of here. An item the name backfill has not reached is its id, held back rather than blank.</summary>
    private static StackPanel SpoilRow(Spoil spoil)
    {
        var stack = new StackPanel { Spacing = 2 };
        var line = new Grid { ColumnSpacing = 8 };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var name = new TextBlock { Text = spoil.Name is { Length: > 0 } known ? known : string.Create(CultureInfo.InvariantCulture, $"Item {spoil.Item}"), TextTrimming = TextTrimming.CharacterEllipsis };
        if (string.IsNullOrEmpty(spoil.Name))
        {
            name.Foreground = Widgets.Tertiary;
        }
        line.Children.Add(name);
        var price = new TextBlock { Text = MarketProse.Gold(spoil.Cheapest), Style = (Style)Application.Current.Resources["Figure"], Foreground = Widgets.AccentBrush };
        Grid.SetColumn(price, 1);
        line.Children.Add(price);
        stack.Children.Add(line);
        stack.Children.Add(Widgets.Text(string.Create(CultureInfo.InvariantCulture, $"{spoil.From} · {spoil.Quantity:N0} listed"), "Caption"));
        return stack;
    }

    /// <summary>Reference material in the platform font, deliberately: the serif is for what is written about the player.</summary>
    private static TextBlock Paragraph(string text) => new()
    {
        Text = text,
        TextWrapping = TextWrapping.Wrap,
        LineHeight = 22,
        Style = (Style)Application.Current.Resources["Secondary"],
    };

    private enum Tone
    {
        Plain,
        Negative,
        Accent,
    }

    /// <summary>A wrapping row of chips.</summary>
    private static Wrap Chips(IEnumerable<(string Text, Tone Tone)> chips)
    {
        var panel = new Wrap { Spacing = 6 };
        foreach (var (text, tone) in chips)
        {
            var label = new TextBlock { Text = text, Style = (Style)Application.Current.Resources["Caption"] };
            if (tone == Tone.Negative)
            {
                label.Foreground = new Microsoft.UI.Xaml.Media.SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xFF, 0x99, 0xA4));
            }
            else if (tone == Tone.Accent)
            {
                label.Foreground = Widgets.AccentBrush;
            }
            panel.Children.Add(new Border
            {
                CornerRadius = new CornerRadius(4),
                Padding = new Thickness(8, 2, 8, 3),
                Background = (Microsoft.UI.Xaml.Media.Brush)Application.Current.Resources["ControlFillColorDefaultBrush"],
                Child = label,
            });
        }
        return panel;
    }
}
