using System.Globalization;
using Armory.Chronicle;
using Armory.Client.Roster;
using Armory.Client.Shell;
using Armory.Roster;
using Armory.Tally;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Armory.App.Pages;

/// <summary>
/// A page a character, reached from a Roster row: header in the class
/// colour, the stat cards, when they play, the gear weakest slot first with
/// the empty slots drawn as empty, and the professions.
/// </summary>
public sealed partial class CharacterPage : Page
{
    /// <summary>How big the portrait is: the GTK page's 52.</summary>
    private const int PortraitSize = 52;

    private readonly Account account;
    private readonly CharacterKey key;
    private readonly MainWindow window;
    private readonly StackPanel column = new() { Spacing = 16 };

    public CharacterPage(Account account, CharacterKey key, MainWindow window)
    {
        this.account = account;
        this.key = key;
        this.window = window;
        Content = new ScrollViewer { Content = column, Padding = (Thickness)Application.Current.Resources["PagePadding"] };
        account.Changed += Redraw;
        Redraw();
    }

    private async void Redraw()
    {
        column.Children.Clear();
        var character = account.Roster.Get(key);
        if (character is null)
        {
            column.Children.Add(Widgets.Header(key.DisplayName(), "No longer on the account"));
            return;
        }
        var detail = account.Details.GetValueOrDefault(key) ?? new Detail();
        var light = Widgets.IsLight(this);

        var crumbs = new BreadcrumbBar { ItemsSource = new[] { "Roster", character.DisplayName } };
        crumbs.ItemClicked += (_, args) =>
        {
            if (args.Index == 0)
            {
                window.Open("roster");
            }
        };
        column.Children.Add(crumbs);

        var theirs = Widgets.Standard("Their evenings");
        theirs.Click += (_, _) => window.OpenChronicle(character.DisplayName);
        var header = Widgets.Header(character.DisplayName, Subtitle(character, detail), theirs);
        ((TextBlock)((StackPanel)header.Children[0]).Children[0]).Foreground = Widgets.ClassBrush(character.Class, light);
        // The portrait before the name, the size the GTK page draws it: small
        // on purpose, because this page is a body of work and not a paper doll.
        header.ColumnDefinitions.Insert(0, new ColumnDefinition { Width = GridLength.Auto });
        foreach (var child in header.Children.OfType<FrameworkElement>())
        {
            Grid.SetColumn(child, Grid.GetColumn(child) + 1);
        }
        var url = account.Portraits.GetValueOrDefault(key) ?? Classes.Crest(account.Settings.Region, character.Class);
        header.Children.Add(Widgets.Portrait(account.Art, url, character.Class, light, PortraitSize));
        column.Children.Add(header);

        // This character's evenings, newest first. No character age exists
        // anywhere, so how long Armory has watched is what the page says.
        var evenings = (await account.Sessions())
            .Where(session => session.Character == key)
            .Select(session => session.Digest())
            .OrderByDescending(digest => digest.StartedAt)
            .ToList();
        var (watched, watchedDetail) = Cards.WatchedFor(evenings, DateTimeOffset.UtcNow);
        column.Children.Add(ChronicleWidgets.Meta($"{watched} · {watchedDetail}"));

        var tallies = await account.Store.On(store => store.TalliesHeld().Match(held => held, _ => []));
        var mine = tallies.GetValueOrDefault(key) ?? [];

        // The GTK page's main column in its order — the strip, the record, the
        // history, the gear, then the hours beside the keys, the vault and the
        // raids — and its rail's sections as cards after it.
        column.Children.Add(CharacterWidgets.Strip(character, detail, evenings));
        column.Children.Add(CharacterWidgets.Record(evenings, mine));
        column.Children.Add(Columns(Widgets.Card(CharacterWidgets.History(evenings)), Widgets.Card(Equipment(detail))));
        column.Children.Add(Columns(Widgets.Card(WhereTheTimeWent(mine)), Widgets.Card(CharacterWidgets.Keys(evenings))));
        column.Children.Add(Columns(Widgets.Card(CharacterWidgets.Vault(detail)), Widgets.Card(CharacterWidgets.Raids(detail))));
        column.Children.Add(Columns(Widgets.Card(Professions(detail)), Widgets.Card(CharacterWidgets.RunShare(character, RunShare())), Widgets.Card(WhenTheyPlay(evenings))));
        column.Children.Add(Columns(Widgets.Card(CharacterWidgets.People(mine)), Widgets.Card(RecentEvening(evenings))));
        column.Children.Add(CharacterWidgets.Footnote(detail));
    }

    /// <summary>Equal columns, one a card, the page's one layout.</summary>
    private static Grid Columns(params FrameworkElement[] cells)
    {
        var grid = new Grid { ColumnSpacing = 16 };
        for (var i = 0; i < cells.Length; i++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            Grid.SetColumn(cells[i], i);
            grid.Children.Add(cells[i]);
        }
        return grid;
    }

    /// <summary>
    /// How much of the run this character can be credited with. A floor, and
    /// the page says so: <c>Run.Credited</c> counts only the goals somebody
    /// attested to or that were measured against one character.
    /// </summary>
    private Share RunShare()
    {
        if (account.Run is not { } held)
        {
            return new Share(0, 0, null);
        }
        var credit = held.Run.Credited();
        var runnerUp = credit
            .Where(pair => pair.Key != key)
            .OrderByDescending(pair => pair.Value)
            .Select(pair => ((string Name, long Count)?)(account.Roster.Get(pair.Key)?.DisplayName ?? pair.Key.Name, pair.Value))
            .FirstOrDefault();
        return new Share(credit.GetValueOrDefault(key), held.Run.Progress().Done, runnerUp);
    }

    /// <summary>The most recent night, which is what belongs to a character rather than to the journal: the chronicle is where every evening lives.</summary>
    private UIElement RecentEvening(List<Digest> evenings)
    {
        var stack = new StackPanel { Spacing = 6 };
        stack.Children.Add(Widgets.CardHead("Most recent evening", evenings.Count == 0 ? null : Account.When(evenings[0].StartedAt, DateTimeOffset.UtcNow)));
        if (evenings.Count == 0)
        {
            stack.Children.Add(Widgets.Text("No evening on this character has been recorded. Install the collector addon and play an evening, and this fills.", "Secondary"));
            return stack;
        }
        var latest = evenings[0];
        var (title, _) = Cards.RoadLine(latest);
        stack.Children.Add(ChronicleWidgets.SerifTitle(latest.Headline(), 17));
        stack.Children.Add(ChronicleWidgets.Meta($"{title} · {Cards.MetaLine(latest, TimeZoneInfo.Local.GetUtcOffset(latest.StartedAt))}"));
        if (latest.Quests.Count > 0)
        {
            stack.Children.Add(Widgets.Text(string.Join(", ", latest.Quests.Take(4).Select(quest => quest.Title)), "Caption"));
        }
        var open = new HyperlinkButton { Content = "Read about it", Padding = new Thickness(0, 4, 0, 0) };
        open.Click += (_, _) => window.OpenChronicle(latest.DisplayName);
        stack.Children.Add(open);
        return stack;
    }

    /// <summary>Seven bars, one a weekday, and the sentence they add up to.</summary>
    private static UIElement WhenTheyPlay(List<Digest> evenings)
    {
        var stack = new StackPanel { Spacing = 8 };
        stack.Children.Add(Widgets.CardHead("When they play", evenings.Count == 0 ? null : Prose.Plural(evenings.Count, "evening", "evenings")));
        if (evenings.Count == 0)
        {
            stack.Children.Add(Widgets.Text("No evenings recorded yet.", "Secondary"));
            return stack;
        }
        var (days, modal, earliest) = Cards.Weekdays(evenings);
        stack.Children.Add(ChronicleWidgets.Weekdays(days, modal));
        stack.Children.Add(Widgets.Text(Cards.WeekdaySentence(modal, earliest), "Caption"));
        return stack;
    }

    private static string Subtitle(Character character, Detail detail)
    {
        var parts = new List<string> { string.Create(CultureInfo.InvariantCulture, $"Level {character.Level} {character.Race} {character.Class}") };
        if (detail.Spec is { Length: > 0 } spec)
        {
            parts.Add(spec);
        }
        if (detail.Guild is { Length: > 0 } guild)
        {
            parts.Add($"<{guild}>");
        }
        parts.Add(character.RealmName);
        return string.Join(" · ", parts);
    }

    private static UIElement WhereTheTimeWent(List<Armory.Tally.Tally> mine)
    {
        var stack = new StackPanel();
        stack.Children.Add(Widgets.CardHead("Where the time went", "hours by zone"));
        var zones = Counters.Of(mine, Counting.Zone).Take(7).ToList();
        if (zones.Count == 0)
        {
            stack.Children.Add(Widgets.Text("Nothing recorded yet. The addon counts the hours as they happen.", "Secondary"));
            return stack;
        }
        var most = zones[0].Count;
        foreach (var zone in zones)
        {
            var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 4, 0, 4) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(110) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(new TextBlock { Text = zone.Label, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis });
            var bar = new ProgressBar { Style = (Style)Application.Current.Resources["ThinBar"], Minimum = 0, Maximum = 1, Value = most == 0 ? 0 : zone.Count / (double)most, VerticalAlignment = VerticalAlignment.Center };
            if (zone != zones[0])
            {
                bar.Opacity = 0.45;
            }
            Grid.SetColumn(bar, 1);
            row.Children.Add(bar);
            var spent = new TextBlock { Text = Counters.Spent(zone.Count), Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Center };
            Grid.SetColumn(spent, 2);
            row.Children.Add(spent);
            stack.Children.Add(row);
        }
        return stack;
    }

    /// <summary>Weakest slot first, because the average hides it. Empty slots are called out, never folded into an average.</summary>
    private static UIElement Equipment(Detail detail)
    {
        var stack = new StackPanel();
        var worn = detail.Equipment;
        var average = detail.ItemLevel is { } level ? string.Create(CultureInfo.InvariantCulture, $"average {level}") : "not read yet";
        stack.Children.Add(Widgets.CardHead("Equipment", average));
        if (worn is null)
        {
            stack.Children.Add(Widgets.Text("No equipment recorded. It arrives with the next sync, or with the collector addon the next time this character logs out.", "Secondary"));
            return stack;
        }
        var byLevel = worn.Where(item => !item.IsCosmetic && item.Level is not null).OrderBy(item => item.Level).ToList();
        var empty = Equipped.Slots.Where(slot => worn.All(item => item.Slot != slot.Slot)).ToList();
        var best = byLevel.Count == 0 ? 1 : byLevel[^1].Level!.Value;
        var first = true;
        foreach (var slot in empty)
        {
            stack.Children.Add(GearRow(slot.Name, "The empty slot", null, best, first, empty: true));
            first = false;
        }
        foreach (var item in byLevel.Take(Math.Max(0, 8 - empty.Count)))
        {
            stack.Children.Add(GearRow(item.SlotName, item.Name, item.Level, best, first));
            first = false;
        }
        if (GearNote(detail, empty.Count) is { } note)
        {
            stack.Children.Add(new TextBlock { Text = note, Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 10, 0, 0) });
        }
        return stack;
    }

    /// <summary>Why the two averages differ, when they do. Nothing when every slot is filled.</summary>
    private static string? GearNote(Detail detail, int empty) => (empty, detail.ItemLevel, detail.EquippedItemLevel) switch
    {
        (0, _, _) => null,
        (_, { } overall, { } equipped) when equipped < overall => string.Create(
            CultureInfo.InvariantCulture,
            $"{(empty == 1 ? "The empty slot" : string.Create(CultureInfo.InvariantCulture, $"{empty} empty slots"))} is why the equipped average is {equipped} and the overall is {overall}. An empty slot is drawn as an empty slot rather than folded into a number."),
        _ => $"{Prose.Plural(empty, "slot is empty", "slots are empty")}. An empty slot is drawn as an empty slot rather than folded into a number.",
    };

    private static Grid GearRow(string slot, string name, int? level, int best, bool weakest, bool empty = false)
    {
        var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 4, 0, 4) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(70) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.Children.Add(new TextBlock { Text = slot, VerticalAlignment = VerticalAlignment.Center });
        var middle = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Spacing = 2 };
        if (weakest)
        {
            middle.Children.Add(new TextBlock { Text = empty ? "the empty slot — the average hides it" : "weakest slot first — the average hides it", Style = (Style)Application.Current.Resources["Caption"] });
        }
        middle.Children.Add(new ProgressBar { Style = (Style)Application.Current.Resources["ThinBar"], Minimum = 0, Maximum = best, Value = level ?? 0 });
        Grid.SetColumn(middle, 1);
        row.Children.Add(middle);
        var figure = new TextBlock { Text = level?.ToString(CultureInfo.InvariantCulture) ?? "—", Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], VerticalAlignment = VerticalAlignment.Center };
        if (empty)
        {
            figure.Foreground = Widgets.Tertiary;
        }
        Grid.SetColumn(figure, 2);
        row.Children.Add(figure);
        ToolTipService.SetToolTip(row, name);
        return row;
    }

    private static UIElement Professions(Detail detail)
    {
        var stack = new StackPanel();
        stack.Children.Add(Widgets.CardHead("Professions"));
        if (detail.Professions.Count == 0)
        {
            stack.Children.Add(Widgets.Text("None recorded.", "Secondary"));
        }
        foreach (var profession in detail.Professions.Where(p => p.IsPrimary).Concat(detail.Professions.Where(p => !p.IsPrimary)))
        {
            var block = new StackPanel { Margin = new Thickness(0, 0, 0, 10) };
            block.Children.Add(new TextBlock { Text = profession.Name, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] });
            var parts = new List<string>();
            if (profession.Tier is { Length: > 0 } tier)
            {
                parts.Add(tier);
            }
            if (profession.Skill is { } skill && profession.MaxSkill is { } max)
            {
                parts.Add(string.Create(CultureInfo.InvariantCulture, $"rank {skill} of {max}"));
            }
            if (profession.Knowledge > 0)
            {
                parts.Add(string.Create(CultureInfo.InvariantCulture, $"{profession.Knowledge} knowledge"));
            }
            block.Children.Add(new TextBlock { Text = string.Join(" — ", parts), Style = (Style)Application.Current.Resources["Caption"] });
            stack.Children.Add(block);
        }
        stack.Children.Add(new TextBlock { Text = "Account-wide counters are not this character's alone.", Style = (Style)Application.Current.Resources["Caption"], Margin = new Thickness(0, 12, 0, 0) });
        return stack;
    }
}
