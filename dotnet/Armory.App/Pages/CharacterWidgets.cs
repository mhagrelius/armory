using System.Globalization;
using Armory.Chronicle;
using Armory.Roster;
using Armory.Tally;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.UI;

namespace Armory.App.Pages;

/// <summary>How a figure is coloured: the accent is "you earned this" and is spent on nothing else; a death is the one negative.</summary>
internal enum Tone
{
    Plain,
    Gold,
    Negative,
}

/// <summary>This character's part of the run: closed goals credited to them, the run's total, and whoever else has the most.</summary>
internal sealed record Share(long Credited, long Closed, (string Name, long Count)? RunnerUp);

/// <summary>One entry on the history list.</summary>
internal sealed record Moment(DateTimeOffset At, string Title, string Detail);

/// <summary>
/// The character page's own sections, ported from <c>ui/character_page.rs</c>:
/// the stat strip, the record, the history, this season's keys, the vault,
/// the raids, the run share, their people and the logout footnote. Each takes
/// the model and returns a card's content; the page decides where it goes.
/// </summary>
internal static class CharacterWidgets
{
    /// <summary>How many evenings the history draws. A summary of a character, not the journal.</summary>
    private const int SpineShown = 4;

    /// <summary>How many companions and questgivers are listed.</summary>
    private const int PeopleShown = 5;

    /// <summary>How many keystone runs the card lists.</summary>
    private const int KeysShown = 6;

    /// <summary>How many raids the card lists, newest first.</summary>
    private const int RaidsShown = 3;

    private static Style Style(string key) => (Style)Application.Current.Resources[key];

    private static Brush Brush(string key) => (Brush)Application.Current.Resources[key];

    private static Brush? Coloured(Tone tone) => tone switch
    {
        Tone.Gold => Widgets.AccentBrush,
        Tone.Negative => Brush("SystemFillColorCriticalBrush"),
        _ => null,
    };

    private static string Thousands(long value) => value.ToString("N0", CultureInfo.InvariantCulture);

    /// <summary>A stat tile, the shape of <see cref="Widgets.Stat"/>, with the figure in the tone it earned.</summary>
    public static Border Tile(string caption, string figure, string note, Tone tone)
    {
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(new TextBlock { Text = caption, Style = Style("Caption") });
        var value = new TextBlock { Text = figure, Style = Style("Figure") };
        if (Coloured(tone) is { } brush)
        {
            value.Foreground = brush;
        }
        stack.Children.Add(value);
        stack.Children.Add(new TextBlock { Text = note, Style = Style("Caption"), TextWrapping = TextWrapping.Wrap });
        return Widgets.Card(stack, dense: true);
    }

    /// <summary>A name on the left and a figure on the right, the almanac's stat line.</summary>
    public static Grid StatLine(string name, string value, Tone tone)
    {
        var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 4, 0, 4) };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.Children.Add(new TextBlock { Text = name, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis });
        var figure = ChronicleWidgets.Figure(value, accent: tone == Tone.Gold);
        if (tone == Tone.Negative)
        {
            figure.Foreground = Brush("SystemFillColorCriticalBrush");
        }
        Grid.SetColumn(figure, 1);
        row.Children.Add(figure);
        return row;
    }

    /// <summary>Small print in the monospace, the almanac's footnote.</summary>
    private static TextBlock Footnote(string text, bool negative = false)
    {
        var block = ChronicleWidgets.Figure(text, size: 12);
        block.Foreground = negative ? Brush("SystemFillColorCriticalBrush") : Widgets.Tertiary;
        return block;
    }

    private static Grid Columns(int count, double spacing = 12)
    {
        var grid = new Grid { ColumnSpacing = spacing };
        for (var i = 0; i < count; i++)
        {
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        }
        return grid;
    }

    /// <summary>Every keystone this character has finished, newest first.</summary>
    public static List<Keystone> Keystones(IReadOnlyList<Digest> evenings) =>
        evenings.SelectMany(evening => evening.Keystones).ToList();

    /// <summary>Four figures, and which of them are theirs. Achievement points are the account's and are deliberately not gold.</summary>
    public static Grid Strip(Character character, Detail detail, IReadOnlyList<Digest> evenings)
    {
        var (itemLevel, itemNote) = (detail.ItemLevel, detail.EquippedItemLevel) switch
        {
            ({ } overall, { } equipped) when equipped < overall => (overall.ToString(CultureInfo.InvariantCulture), string.Create(CultureInfo.InvariantCulture, $"{equipped} equipped — a slot is empty")),
            ({ } overall, _) => (overall.ToString(CultureInfo.InvariantCulture), "every slot filled"),
            _ => ("—", "not reported"),
        };
        var keys = Keystones(evenings).Count;
        var (rating, ratingNote) = detail.MythicRating is { } best
            ? (best.ToString(CultureInfo.InvariantCulture), $"this season · {Prose.Plural(keys, "key", "keys")}")
            : ("—", "no rating this season");
        var hours = evenings.Sum(evening => Math.Max((long)evening.Duration.TotalMinutes, 0)) / 60;

        var strip = Columns(4);
        var tiles = new[]
        {
            Tile("Item level", itemLevel, itemNote, Tone.Gold),
            Tile("Mythic+ rating", rating, ratingNote, Tone.Gold),
            Tile("Achievement points", detail.AchievementPoints is { } points ? Thousands(points) : "—", $"account-wide, not {character.DisplayName}'s alone", Tone.Plain),
            Tile("Hours watched", hours.ToString(CultureInfo.InvariantCulture), evenings.Count == 0 ? "nothing recorded yet" : Prose.Plural(evenings.Count, "evening", "evenings"), Tone.Gold),
        };
        for (var i = 0; i < tiles.Length; i++)
        {
            Grid.SetColumn(tiles[i], i);
            strip.Children.Add(tiles[i]);
        }
        return strip;
    }

    /// <summary>The record: six lifetime figures. Deaths are the one negative; what they earned is the one gold.</summary>
    public static StackPanel Record(IReadOnlyList<Digest> evenings, IReadOnlyList<Tally.Tally> tallies)
    {
        long Count(Counting kind) => tallies.Where(tally => tally.Kind == kind).Sum(tally => tally.Count);
        var quests = evenings.Sum(evening => evening.Quests.Count);
        var gold = evenings.Sum(evening => evening.Income.Sum(income => income.Amount));

        var lines = new (string Name, string Value, Tone Tone)[]
        {
            ("Quests turned in", Thousands(quests), Tone.Plain),
            ("Bosses beaten", Thousands(Count(Counting.Victory)), Tone.Plain),
            ("Deaths", Thousands(Count(Counting.Killer)), Tone.Negative),
            ("Distance covered", $"{Thousands(Count(Counting.Distance) / 1760)} miles", Tone.Plain),
            ("Flights taken", Thousands(Count(Counting.Flight)), Tone.Plain),
            ("Earned, all sources", $"{Thousands(gold / 10_000)}g", Tone.Gold),
        };
        var stack = new StackPanel { Spacing = 8 };
        var grid = Columns(lines.Length);
        for (var i = 0; i < lines.Length; i++)
        {
            var tile = Tile(lines[i].Name, lines[i].Value, "", lines[i].Tone);
            Grid.SetColumn(tile, i);
            grid.Children.Add(tile);
        }
        stack.Children.Add(grid);
        if (tallies.Count == 0)
        {
            stack.Children.Add(Widgets.Text("These are the addon's counters. Nothing in the game or the API can give them back, so they start the day it is installed.", "Caption"));
        }
        return stack;
    }

    /// <summary>
    /// The evenings worth putting on a spine, newest first. Firsts and lasts
    /// rather than the last four evenings: the chronicle is where every
    /// evening lives, and repeating its top four here would be the same list
    /// drawn twice.
    /// </summary>
    public static List<Moment> Moments(IReadOnlyList<Digest> evenings)
    {
        var moments = new List<Moment>();
        static string Length(Digest evening)
        {
            var minutes = Math.Max((long)evening.Duration.TotalMinutes, 0);
            return string.Create(CultureInfo.InvariantCulture, $"{minutes / 60}h {minutes % 60:00}m");
        }

        if (evenings.Count > 0)
        {
            var latest = evenings[0];
            var parts = new List<string> { Length(latest) };
            if (latest.Quests.Count > 0)
            {
                parts.Add(Prose.Plural(latest.Quests.Count, "quest", "quests"));
            }
            if (latest.Deaths.Count > 0)
            {
                parts.Add(Prose.Plural(latest.Deaths.Count, "death", "deaths"));
            }
            moments.Add(new Moment(latest.StartedAt, latest.Route.Count > 0 ? latest.Route[0].Zone : "The most recent evening", $"Their most recent evening — {string.Join(", ", parts)}."));
        }

        // The highest level reached, and the night it happened. A character
        // levels once and it is the fact they are defined by afterwards.
        var levelled = evenings
            .SelectMany(evening => evening.Levels.Select(level => (evening.StartedAt, level.Level, level.Zone)))
            .OrderByDescending(reached => reached.Level)
            .FirstOrDefault();
        if (levelled.Zone is not null)
        {
            moments.Add(new Moment(levelled.StartedAt, string.Create(CultureInfo.InvariantCulture, $"Reached level {levelled.Level}"), $"Standing in {levelled.Zone}."));
        }

        if (evenings.Count > 1)
        {
            var first = evenings[^1];
            var where = first.Route.Count > 0 ? first.Route[0].Zone : "Somewhere unrecorded";
            moments.Add(new Moment(first.StartedAt, "First evening Armory watched", $"{where}, {Length(first)}. Nothing before this is recorded — not by Armory and not by anything else."));
        }

        return moments
            .OrderByDescending(moment => moment.At)
            .DistinctBy(moment => (moment.At, moment.Title))
            .Take(SpineShown)
            .ToList();
    }

    /// <summary>The dated list: the evenings that were firsts.</summary>
    public static StackPanel History(IReadOnlyList<Digest> evenings)
    {
        var stack = new StackPanel();
        stack.Children.Add(Widgets.CardHead("Their own history"));
        var moments = Moments(evenings);
        if (moments.Count == 0)
        {
            stack.Children.Add(Widgets.Text("Nothing recorded on this character yet. Install the collector addon and play an evening, and this fills.", "Secondary"));
            return stack;
        }
        var newest = true;
        foreach (var moment in moments)
        {
            var row = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 6, 0, 6) };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(56) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

            var local = moment.At.ToLocalTime();
            var date = new StackPanel { HorizontalAlignment = HorizontalAlignment.Right };
            var day = ChronicleWidgets.Figure(local.ToString("dd MMM", CultureInfo.InvariantCulture).ToUpperInvariant(), accent: newest, size: 12);
            day.HorizontalAlignment = HorizontalAlignment.Right;
            date.Children.Add(day);
            var year = Footnote(local.ToString("yyyy", CultureInfo.InvariantCulture));
            year.HorizontalAlignment = HorizontalAlignment.Right;
            date.Children.Add(year);
            row.Children.Add(date);

            var dot = new Ellipse
            {
                Width = 8,
                Height = 8,
                Margin = new Thickness(0, 6, 0, 0),
                VerticalAlignment = VerticalAlignment.Top,
                Fill = newest ? Widgets.AccentBrush : new SolidColorBrush(Color.FromArgb(92, 255, 255, 255)),
            };
            Grid.SetColumn(dot, 1);
            row.Children.Add(dot);

            var text = new StackPanel { Spacing = 2 };
            text.Children.Add(new TextBlock { Text = moment.Title, Style = Style("BodyStrongTextBlockStyle"), TextWrapping = TextWrapping.Wrap });
            text.Children.Add(new TextBlock { Text = moment.Detail, Style = Style("Caption"), TextWrapping = TextWrapping.Wrap });
            Grid.SetColumn(text, 2);
            row.Children.Add(text);

            stack.Children.Add(row);
            newest = false;
        }
        return stack;
    }

    /// <summary>This season's keystones, as the addon saw them run.</summary>
    public static StackPanel Keys(IReadOnlyList<Digest> evenings)
    {
        var stack = new StackPanel { Spacing = 4 };
        stack.Children.Add(Widgets.CardHead("This season's keys"));
        var runs = Keystones(evenings);
        if (runs.Count == 0)
        {
            stack.Children.Add(Widgets.Text("No keystone finished on this character while the addon was watching. The rating above comes from Blizzard and covers the whole season; these are the runs Armory saw.", "Secondary"));
        }
        var first = true;
        foreach (var key in runs.Take(KeysShown))
        {
            if (!first)
            {
                stack.Children.Add(ChronicleWidgets.Hairline());
            }
            first = false;
            var row = new Grid { ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(new TextBlock { Text = key.Dungeon, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis });
            var level = ChronicleWidgets.Figure(string.Create(CultureInfo.InvariantCulture, $"+{key.Level}"), accent: true);
            Grid.SetColumn(level, 1);
            row.Children.Add(level);
            // An untimed key is a real outcome rather than a failure to hide,
            // so it is stated in the same place the timed ones are.
            var outcome = key.InTime ? Footnote("TIMED") : Footnote("OVER", negative: true);
            Grid.SetColumn(outcome, 2);
            row.Children.Add(outcome);
            stack.Children.Add(row);
        }
        if (runs.Count > 0)
        {
            stack.Children.Add(ChronicleWidgets.Hairline());
            var best = runs.Max(key => key.Level);
            stack.Children.Add(StatLine($"{Prose.Plural(runs.Count, "key", "keys")} recorded", string.Create(CultureInfo.InvariantCulture, $"BEST +{best}"), Tone.Gold));
            // The empty state above already carries this sentence.
            stack.Children.Add(new TextBlock { Text = "The rating above comes from Blizzard and covers the whole season; these are the runs Armory saw.", Style = Style("Caption"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 0) });
        }
        return stack;
    }

    /// <summary>
    /// The Great Vault as the client last saw it. There is no endpoint for
    /// this — the weekly frame is client-side state — so it comes from the
    /// addon or not at all, and the card says which.
    /// </summary>
    public static StackPanel Vault(Detail detail)
    {
        var stack = new StackPanel { Spacing = 4 };
        stack.Children.Add(Widgets.CardHead("The Vault"));
        if (detail.Vault is not { } slots)
        {
            stack.Children.Add(Widgets.Text("There is no Great Vault endpoint — the weekly frame is client-side state. The collector addon reads it at logout; nothing has arrived for this character yet.", "Secondary"));
            return stack;
        }
        if (detail.VaultReady)
        {
            var chip = ChronicleWidgets.Chip("REWARD WAITING", accent: true);
            chip.HorizontalAlignment = HorizontalAlignment.Left;
            chip.Margin = new Thickness(0, 0, 0, 6);
            stack.Children.Add(chip);
        }
        VaultRow? lastRow = null;
        foreach (var slot in slots)
        {
            if (lastRow is { } was && was != slot.Row)
            {
                stack.Children.Add(ChronicleWidgets.Hairline());
            }
            lastRow = slot.Row;
            var row = new Grid { ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(72) });
            row.Children.Add(new TextBlock { Text = slot.Row.Label(), VerticalAlignment = VerticalAlignment.Center });
            var count = Footnote(string.Create(CultureInfo.InvariantCulture, $"{slot.Progress}/{slot.Threshold}"));
            Grid.SetColumn(count, 1);
            row.Children.Add(count);
            // An unlocked slot names what it pays; a locked one is a count and
            // nothing to promise.
            var reward = slot.IsUnlocked ? ChronicleWidgets.Figure(slot.Reward().ToUpperInvariant(), accent: true) : Footnote("LOCKED");
            reward.HorizontalAlignment = HorizontalAlignment.Right;
            Grid.SetColumn(reward, 2);
            row.Children.Add(reward);
            stack.Children.Add(row);
        }
        stack.Children.Add(new TextBlock { Text = "From the game client at this character's last logout. A slot that filled on another character's evening is not here until this one logs in again.", Style = Style("Caption"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 0) });
        return stack;
    }

    /// <summary>Raid progress from the API, newest tier first; this week's lockouts from the client when the API has said nothing.</summary>
    public static StackPanel Raids(Detail detail)
    {
        var stack = new StackPanel { Spacing = 10 };
        stack.Children.Add(Widgets.CardHead("Raids"));
        if (detail.Raids is { Count: > 0 } tiers)
        {
            // Newest last in Blizzard's ordering, so the current tier is at
            // the end and the page reads the other way.
            var current = true;
            foreach (var tier in tiers.AsEnumerable().Reverse().Take(RaidsShown))
            {
                stack.Children.Add(RaidCard(tier, current));
                current = false;
            }
            return stack;
        }
        stack.Children.Add(Lockouts(detail));
        return stack;
    }

    private static Border RaidCard(RaidTier tier, bool current)
    {
        var body = new StackPanel { Spacing = 6 };
        var head = new Grid { ColumnSpacing = 8 };
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        head.Children.Add(new TextBlock { Text = tier.Name, Style = Style("BodyStrongTextBlockStyle"), TextWrapping = TextWrapping.Wrap });
        if (current)
        {
            var chip = ChronicleWidgets.Chip("CURRENT", accent: true);
            Grid.SetColumn(chip, 1);
            head.Children.Add(chip);
        }
        body.Children.Add(head);

        foreach (var difficulty in tier.Difficulties)
        {
            var row = new Grid { ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(96) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(Footnote(difficulty.Name.ToUpperInvariant()));
            var bar = Widgets.Bar(difficulty.Defeated / (double)Math.Max(difficulty.Total, 1));
            bar.VerticalAlignment = VerticalAlignment.Center;
            Grid.SetColumn(bar, 1);
            row.Children.Add(bar);
            var count = ChronicleWidgets.Figure(string.Create(CultureInfo.InvariantCulture, $"{difficulty.Defeated}/{difficulty.Total}"), accent: true);
            Grid.SetColumn(count, 2);
            row.Children.Add(count);
            body.Children.Add(row);
        }

        if (tier.LastKill() is { } kill)
        {
            body.Children.Add(new TextBlock
            {
                Text = $"Last kill {kill.At.ToLocalTime().ToString("d MMMM", CultureInfo.InvariantCulture)} — {kill.Boss}, {kill.Difficulty.ToLowerInvariant()}.",
                Style = Style("Caption"),
                TextWrapping = TextWrapping.Wrap,
            });
        }

        var card = Widgets.Card(body, dense: true);
        if (current)
        {
            card.BorderBrush = Widgets.AccentBrush;
            card.BorderThickness = new Thickness(1);
        }
        return card;
    }

    /// <summary>What the client knows when the web API has said nothing.</summary>
    private static StackPanel Lockouts(Detail detail)
    {
        var stack = new StackPanel { Spacing = 4 };
        if (detail.RaidLocks is not { } locks)
        {
            stack.Children.Add(Widgets.Text("No raid progress recorded. It arrives with a sync, or with the collector addon the next time this character logs out.", "Secondary"));
            return stack;
        }
        if (locks.Count == 0)
        {
            stack.Children.Add(Widgets.Text("Not saved to any raid this week. The lifetime record comes from Blizzard's API, which has not answered for this character.", "Secondary"));
            return stack;
        }
        var first = true;
        foreach (var raidLock in locks)
        {
            if (!first)
            {
                stack.Children.Add(ChronicleWidgets.Hairline());
            }
            first = false;
            var row = new Grid { ColumnSpacing = 12 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(new TextBlock { Text = raidLock.Name, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis });
            var difficulty = Footnote(raidLock.Difficulty.ToUpperInvariant());
            Grid.SetColumn(difficulty, 1);
            row.Children.Add(difficulty);
            var count = ChronicleWidgets.Figure(string.Create(CultureInfo.InvariantCulture, $"{raidLock.Defeated}/{raidLock.Total}"), accent: true);
            Grid.SetColumn(count, 2);
            row.Children.Add(count);
            stack.Children.Add(row);
        }
        // Said plainly, because these two facts look alike and are not.
        stack.Children.Add(new TextBlock { Text = "This week's lockouts, from the game client — not a lifetime. The client cannot see what it has ever killed and the API can, so this is what an account with no Battle.net client has.", Style = Style("Caption"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 0) });
        return stack;
    }

    /// <summary>
    /// A floor, and said so. Most of a run is account-wide work nothing can pin
    /// on one character, and sharing it out evenly would invent an answer
    /// nobody measured.
    /// </summary>
    public static StackPanel RunShare(Character character, Share share)
    {
        var stack = new StackPanel { Spacing = 6 };
        stack.Children.Add(Widgets.CardHead("Their share of the run"));
        stack.Children.Add(new TextBlock { Text = share.Credited.ToString(CultureInfo.InvariantCulture), Style = Style("LargeFigure"), Foreground = Widgets.AccentBrush });
        stack.Children.Add(new TextBlock { Text = $"of {Thousands(share.Closed)} closed", Style = Style("Caption") });
        stack.Children.Add(Widgets.Bar(share.Credited / (double)Math.Max(share.Closed, 1)));
        var sentence = share.RunnerUp switch
        {
            ({ } name, var count) when count > share.Credited =>
                string.Create(CultureInfo.InvariantCulture, $"{name} has more, with {count}. Only goals somebody attested to or that were measured against one character are credited at all — the rest is the account's."),
            ({ } name, var count) =>
                string.Create(CultureInfo.InvariantCulture, $"The most of any character in the cohort. {name} is second with {count}. Only goals somebody attested to or that were measured against one character are credited at all."),
            _ => $"Only goals somebody attested to, or that were measured against {character.DisplayName}, are credited at all — the rest is the account's.",
        };
        stack.Children.Add(new TextBlock { Text = sentence, Style = Style("Caption"), TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 4, 0, 0) });
        return stack;
    }

    /// <summary>Who they play with, and who sends them out.</summary>
    public static StackPanel People(IReadOnlyList<Tally.Tally> tallies)
    {
        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(Widgets.CardHead("Their people"));
        List<Tally.Tally> Listed(Counting kind) => tallies.Where(tally => tally.Kind == kind).OrderByDescending(tally => tally.Count).Take(PeopleShown).ToList();
        var companions = Listed(Counting.Companion);
        var questgivers = Listed(Counting.Questgiver);
        if (companions.Count == 0 && questgivers.Count == 0)
        {
            stack.Children.Add(Widgets.Text("Nobody recorded yet. The addon counts who is in the party and who hands over a quest; nothing else does.", "Secondary"));
            return stack;
        }
        foreach (var tally in companions)
        {
            stack.Children.Add(StatLine(tally.Label, Prose.Plural(tally.Count, "evening", "evenings"), Tone.Plain));
        }
        if (companions.Count > 0 && questgivers.Count > 0)
        {
            stack.Children.Add(ChronicleWidgets.Hairline());
        }
        foreach (var tally in questgivers)
        {
            stack.Children.Add(StatLine(tally.Label, Prose.Plural(tally.Count, "quest", "quests"), Tone.Plain));
        }
        return stack;
    }

    /// <summary>When everything Blizzard reports here was true, said once at the foot of the page.</summary>
    public static StackPanel Footnote(Detail detail)
    {
        var stack = new StackPanel { Spacing = 6 };
        stack.Children.Add(ChronicleWidgets.Hairline());
        stack.Children.Add(Widgets.Text("Everything Blizzard reports here was true when this character last logged out. The counters and the evenings come from the addon and are true as of the last one it recorded.", "Caption"));
        if (detail.LastLogin is { } last)
        {
            stack.Children.Add(ChronicleWidgets.Meta($"TRUE AT LOGOUT · {last.ToLocalTime().ToString("d MMM yyyy HH:mm", CultureInfo.InvariantCulture)}"));
        }
        return stack;
    }
}
