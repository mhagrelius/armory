using System.Globalization;
using Armory.Chronicle;
using Armory.Client.Roster;
using Armory.Client.Shell;
using Armory.Roster;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;

namespace Armory.App.Pages;

/// <summary>Which characters the run is about: a table with an enrolment toggle per row, and the account's totals as cards beneath.</summary>
public sealed partial class RosterPage : Page, ISearchable
{
    /// <summary>The portrait's side, in the page's units. The ring is drawn at the same size over it.</summary>
    private const int PortraitSize = 36;

    private readonly Account account;
    private readonly MainWindow window;
    private readonly StackPanel column = new() { Spacing = 16 };
    private string needle = "";
    private bool enrolledOnly;

    public RosterPage(Account account, MainWindow window)
    {
        this.account = account;
        this.window = window;
        Content = new ScrollViewer { Content = column, Padding = (Thickness)Application.Current.Resources["PagePadding"] };
        account.Changed += Redraw;
        Redraw();
    }

    public string SearchPlaceholder => "Search characters, realms and classes";

    public void Search(string text)
    {
        needle = text.Trim();
        Redraw();
    }

    private async void Redraw()
    {
        column.Children.Clear();
        var roster = account.Roster;
        var cohort = account.Cohort;
        var details = account.Details;
        var enrolled = cohort.Count;

        var sync = Widgets.Accent("Sync", "");
        // Battle.net, as the GTK's `app.sync`; sharing with the NAS runs on its own clock.
        sync.Click += async (_, _) => await account.Sync();
        var headline = roster.IsEmpty ? "Nobody on the account yet" : RosterProse.Headline(enrolled, roster.Count);
        column.Children.Add(Widgets.Header("Roster", headline, sync));

        var selector = new SelectorBar();
        var all = new SelectorBarItem { Text = "Characters", IsSelected = !enrolledOnly };
        var only = new SelectorBarItem { Text = "Enrolled", IsSelected = enrolledOnly };
        selector.Items.Add(all);
        selector.Items.Add(only);
        selector.SelectionChanged += (sender, _) =>
        {
            // Raised during the bar's first measure as well as on a click;
            // rebuilding the page inside a measure pass is fatal, so only a
            // change redraws, and on the next dispatcher turn.
            var wanted = sender.SelectedItem == only;
            if (wanted == enrolledOnly)
            {
                return;
            }
            enrolledOnly = wanted;
            DispatcherQueue.TryEnqueue(Redraw);
        };
        column.Children.Add(selector);

        if (roster.IsEmpty)
        {
            column.Children.Add(Widgets.Standing(
                "No characters yet",
                "Log a character out with the collector addon installed, or sign in with a Battle.net client, and the roster fills in.",
                InfoBarSeverity.Informational));
        }

        var table = new StackPanel();
        table.Children.Add(TableHead());
        var light = Widgets.IsLight(this);
        var shown = roster.Characters
            .Where(character => !enrolledOnly || cohort.Contains(character.Key))
            .Where(character => RosterProse.Matches(character, details.GetValueOrDefault(character.Key), needle))
            .ToList();
        foreach (var character in shown)
        {
            table.Children.Add(Row(character, details.GetValueOrDefault(character.Key), cohort.Contains(character.Key), light));
        }
        if (shown.Count == 0 && !roster.IsEmpty)
        {
            table.Children.Add(Widgets.Text("No character, realm or class here matches that.", "Secondary"));
        }
        table.Children.Add(new TextBlock
        {
            Text = "Only enrolled characters count towards a run. The rest are kept only to explain why something is already spent.",
            Style = (Style)Application.Current.Resources["Caption"],
            Margin = new Thickness(0, 10, 0, 0),
        });
        column.Children.Add(Widgets.Card(table));

        var warbandHeld = await account.Warband();
        var cards = new List<Border>();
        var atCap = roster.Characters.Count(character => character.Level >= 80);
        cards.Add(Widgets.Stat(
            "Characters",
            Prose.Thousands(roster.Count),
            string.Create(CultureInfo.InvariantCulture, $"{enrolled} enrolled · {atCap} at level cap")));
        // Only for what has actually been fetched. Gold comes from a separate
        // call per character and only for the enrolled ones, so a total across
        // an unsynced roster would be a confident understatement — which is
        // what the note is there to say.
        var gold = details.Values.Sum(detail => detail.Money ?? 0);
        if (gold > 0)
        {
            cards.Add(Widgets.Stat("Gold", Gold(gold), "Across the characters whose detail has been fetched, which is the enrolled ones. Not a total for the account."));
        }
        cards.Add(Widgets.Stat(
            "Warband bank",
            warbandHeld.BankItems == 0 ? "Not scanned yet" : string.Create(CultureInfo.InvariantCulture, $"{warbandHeld.BankItems:N0} items"),
            warbandHeld.BankItems == 0 ? "Stand at the bank in game with the addon on" : "Shown, never subtracted from a craft's cost"));
        cards.Add(Widgets.Stat(
            "Currencies",
            warbandHeld.Currencies == 0 ? "None yet" : string.Create(CultureInfo.InvariantCulture, $"{warbandHeld.Currencies:N0} tracked"),
            warbandHeld.Currencies == 0
                ? "Not scanned yet — visit a banker in game"
                : string.Create(CultureInfo.InvariantCulture, $"{warbandHeld.EarnedCurrencies:N0} earned · {warbandHeld.TransferredCurrencies:N0} transferred · {warbandHeld.UnclearCurrencies:N0} unclear")));
        cards.Add(Widgets.Stat("Profiles", "Written at logout", "Blizzard writes a profile only when a character logs out"));

        var stats = new Grid { ColumnSpacing = 16, RowSpacing = 16 };
        for (var i = 0; i < 3; i++)
        {
            stats.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        }
        for (var i = 0; i < cards.Count; i++)
        {
            if (i % 3 == 0)
            {
                stats.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            }
            Grid.SetRow(cards[i], i / 3);
            Grid.SetColumn(cards[i], i % 3);
            stats.Children.Add(cards[i]);
        }
        column.Children.Add(stats);
    }

    private static Grid Columns()
    {
        var grid = new Grid { ColumnSpacing = 12 };
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(60) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(64) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(120) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(100) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(92) });
        return grid;
    }

    private static Grid TableHead()
    {
        var head = Columns();
        head.Margin = new Thickness(0, 0, 0, 6);
        var labels = new[] { "Character", "Level", "ILVL", "Mythic+ rating", "Gold", "In the run" };
        for (var i = 0; i < labels.Length; i++)
        {
            var label = new TextBlock { Text = labels[i], Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = i >= 1 ? HorizontalAlignment.Right : HorizontalAlignment.Left };
            Grid.SetColumn(label, i);
            head.Children.Add(label);
        }
        return head;
    }

    private Border Row(Character character, Detail? detail, bool enrolled, bool light)
    {
        var grid = Columns();
        var who = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        who.Children.Add(Portrait(character, light));
        var name = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        var title = new TextBlock { Text = character.DisplayName, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], Foreground = Widgets.ClassBrush(character.Class, light) };
        name.Children.Add(title);
        name.Children.Add(new TextBlock { Text = RosterProse.Subtitle(character, detail), Style = (Style)Application.Current.Resources["Caption"], TextTrimming = TextTrimming.CharacterEllipsis });
        who.Children.Add(name);
        grid.Children.Add(who);

        var level = Figure(character.Level.ToString(CultureInfo.InvariantCulture));
        if (character.Level < 80)
        {
            level.Foreground = Widgets.Tertiary;
        }
        Grid.SetColumn(level, 1);
        grid.Children.Add(level);

        var itemLevel = Figure(detail?.ItemLevel is { } ilvl ? ilvl.ToString(CultureInfo.InvariantCulture) : "—");
        Grid.SetColumn(itemLevel, 2);
        grid.Children.Add(itemLevel);

        var rating = Figure(detail?.MythicRating is { } score ? Prose.Thousands(score) : "—");
        Grid.SetColumn(rating, 3);
        grid.Children.Add(rating);

        var gold = Figure(detail?.Money is { } copper ? Gold(copper) : "—");
        Grid.SetColumn(gold, 4);
        grid.Children.Add(gold);

        var toggle = new ToggleSwitch { IsOn = enrolled, OnContent = "", OffContent = "", MinWidth = 0, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Center };
        ToolTipService.SetToolTip(toggle, enrolled ? "Take this character out of the run" : "Measure this character's progress as part of the run");
        var key = character.Key;
        toggle.Toggled += async (_, _) => await account.ToggleEnrolment(key);
        Grid.SetColumn(toggle, 5);
        grid.Children.Add(toggle);

        var row = new Border { Style = (Style)Application.Current.Resources["RowDivider"], Child = grid };
        // Specialisation trees stay in the tooltip. They are real information
        // with no endpoint behind them, and two professions carrying a tree
        // name each is more than a row has room for.
        if (detail is not null && RosterProse.Depths(detail) is { } more)
        {
            ToolTipService.SetToolTip(row, more);
        }
        // A name opens the character page, reached from the roster.
        ToolTipService.SetToolTip(title, $"Open {character.DisplayName}");
        title.PointerPressed += (_, _) => window.OpenCharacter(key);
        title.PointerEntered += (_, _) => ProtectedCursor = InputSystemCursor.Create(InputSystemCursorShape.Hand);
        title.PointerExited += (_, _) => ProtectedCursor = null;
        return row;
    }

    private static TextBlock Figure(string text) => new() { Text = text, HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Center };

    /// <summary>
    /// The character's own render if we have one, and their class crest if
    /// not, inside a ring in the class colour. The placeholder is the same
    /// grey box every other piece of art starts as, rounded to fit the ring.
    /// </summary>
    private Grid Portrait(Character character, bool light)
    {
        var url = account.Portraits.GetValueOrDefault(character.Key) ?? Classes.Crest(account.Settings.Region, character.Class);
        return Widgets.Portrait(account.Art, url, character.Class, light, PortraitSize, RosterProse.Who(character));
    }

    internal static string Gold(long copper) => string.Create(CultureInfo.InvariantCulture, $"{Prose.Thousands(copper / 10_000)}g");
}
