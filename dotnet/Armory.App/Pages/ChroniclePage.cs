using System.Globalization;
using Armory.Chronicle;
using Armory.Client.Shell;
using Armory.Roster;
using Armory.Tally;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;

namespace Armory.App.Pages;

/// <summary>
/// A journal, one evening a row: the handoff's list-detail Chronicle. The
/// list is every evening worth showing, newest first; the detail is the
/// entry a local model wrote on top of the evening's own working out, and
/// under it the lifetime counters, drawn shut, for the character it belongs
/// to. Nothing here was fetched from anywhere: the addon recorded it, the
/// model on this machine wrote it up.
/// </summary>
public sealed partial class ChroniclePage : Page, ISearchable
{
    private const int OverheardShown = 5;
    private const int ToldShown = 3;
    private const int RouteShown = 4;
    private const int TallyShown = 6;

    private readonly Account account;
    private readonly MainWindow window;
    private readonly Grid root = new() { RowSpacing = 12 };
    private readonly StackPanel head = new();
    private readonly ListView list = new() { SelectionMode = ListViewSelectionMode.Single, Padding = new Thickness(0, 4, 0, 4) };
    private readonly StackPanel detail = new() { Spacing = 14 };
    private readonly ScrollViewer detailScroller;
    private List<Session> sessions = [];
    private List<Digest> digests = [];
    private Dictionary<SessionId, Entry> entries = [];
    private List<(DateTimeOffset At, string Path)> shots = [];
    private Tallies tallies = [];
    private string needle = "";
    private string showing = "All";
    private string? only;
    private SessionId? selected;
    private bool reloading;
    private bool reloadAgain;
    private bool loaded;

    public ChroniclePage(Account account, MainWindow window)
    {
        this.account = account;
        this.window = window;
        root.Padding = (Thickness)Application.Current.Resources["PagePadding"];
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.Children.Add(head);

        var body = new Grid { ColumnSpacing = 16 };
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1.25, GridUnitType.Star) });
        var listCard = Widgets.Card(list, dense: true);
        listCard.VerticalAlignment = VerticalAlignment.Stretch;
        body.Children.Add(listCard);
        detailScroller = new ScrollViewer { Content = detail, Padding = new Thickness(0, 0, 8, 0) };
        Grid.SetColumn(detailScroller, 1);
        body.Children.Add(detailScroller);
        Grid.SetRow(body, 1);
        root.Children.Add(body);
        Content = root;

        list.SelectionChanged += (_, _) =>
        {
            // Fires during the first measure when a row is born selected;
            // nothing may be rebuilt inline there.
            var chosen = (list.SelectedItem as ListViewItem)?.Tag as SessionId;
            if (chosen is null || chosen == selected)
            {
                return;
            }
            selected = chosen;
            _ = DispatcherQueue.TryEnqueue(DrawDetail);
        };

        // The header at once, so the page is not blank while the first read
        // queues behind an addon write on the store's one thread.
        DrawHead();
        account.Changed += Reload;
        Reload();
    }

    public string SearchPlaceholder => "Search entries, quests and places";

    public void Search(string text)
    {
        needle = text.Trim();
        DrawList();
    }

    /// <summary>Show one character's evenings, the way the Character page's "Their evenings" arrives here.</summary>
    public void ShowOnly(string? displayName)
    {
        only = displayName;
        DrawAll();
    }

    private async void Reload()
    {
        // A change that lands while a read is in flight is not dropped: the
        // read runs again, because what it was reading has moved.
        if (reloading)
        {
            reloadAgain = true;
            return;
        }
        reloading = true;
        try
        {
            do
            {
                reloadAgain = false;
                sessions = await account.Sessions();
                entries = await account.Entries();
                tallies = await account.Store.On(store => store.TalliesHeld().Match(held => held, _ => []));
                // Only evenings worth showing. Logging in to post an auction
                // is recorded, because taking it back out later is
                // impossible, but a journal whose first screen is nine of
                // those is one nobody opens twice.
                digests = sessions.Select(session => session.Digest()).Where(digest => digest.IsWorthWriting()).OrderByDescending(digest => digest.StartedAt).ToList();
                shots = sessions.Count == 0 ? [] : account.Shots(sessions.Min(session => session.StartedAt));
            }
            while (reloadAgain);
        }
        finally
        {
            reloading = false;
        }
        loaded = true;
        DrawAll();
    }

    private void DrawAll()
    {
        DrawHead();
        DrawList();
    }

    private void DrawHead()
    {
        head.Children.Clear();
        var written = digests.Count(digest => entries.ContainsKey(digest.Id));
        var subtitle = !loaded
            ? "Reading the journal…"
            : digests.Count == 0
                ? "Nothing recorded yet"
                : string.Create(CultureInfo.InvariantCulture, $"{Prose.Plural(digests.Count, "evening", "evenings")} recorded · {written} written up");

        var write = Widgets.Accent("Write entry", "");
        write.IsEnabled = account.JournalReady && selected is not null && !account.IsWriting(selected);
        write.Click += async (_, _) =>
        {
            if (selected is { } id)
            {
                await account.WriteEntry(id);
            }
        };
        var setup = Widgets.Standard("Set up the journal");
        setup.Click += async (_, _) => await Setup();
        var more = new Button { Content = new FontIcon { Glyph = "", FontSize = 16 }, MinHeight = 34 };
        var flyout = new MenuFlyout();
        var all = new MenuFlyoutItem { Text = "Write every entry" };
        all.Click += async (_, _) =>
        {
            if (!await account.WriteAll())
            {
                await Setup();
            }
        };
        flyout.Items.Add(all);
        var forget = new MenuFlyoutItem { Text = "Forget this evening…", IsEnabled = selected is not null };
        forget.Click += async (_, _) => await ConfirmForget();
        flyout.Items.Add(forget);
        more.Flyout = flyout;
        head.Children.Add(Widgets.Header("Chronicle", subtitle, write, setup, more));

        var controls = new Grid { ColumnSpacing = 12, Margin = new Thickness(0, 0, 0, 4) };
        controls.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        controls.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var selector = new SelectorBar();
        foreach (var name in new[] { "All", "Unwritten", "Written" })
        {
            selector.Items.Add(new SelectorBarItem { Text = name, IsSelected = name == showing });
        }
        selector.SelectionChanged += (sender, args) =>
        {
            var chosen = sender.SelectedItem?.Text ?? "All";
            if (chosen == showing)
            {
                return;
            }
            showing = chosen;
            _ = DispatcherQueue.TryEnqueue(DrawList);
        };
        controls.Children.Add(selector);

        // "Everyone" is not a character. The dropdown hides itself on an
        // account where only one has ever played.
        var names = sessions.Select(session => session.DisplayName).Distinct(StringComparer.Ordinal).OrderBy(name => name, StringComparer.Ordinal).ToList();
        if (names.Count > 1)
        {
            var who = new ComboBox { MinWidth = 160, HorizontalAlignment = HorizontalAlignment.Right };
            who.Items.Add("Everyone");
            foreach (var name in names)
            {
                who.Items.Add(name);
            }
            who.SelectedIndex = only is { } chosen && names.Contains(chosen) ? names.IndexOf(chosen) + 1 : 0;
            who.SelectionChanged += (_, _) =>
            {
                var picked = who.SelectedIndex <= 0 ? null : names[who.SelectedIndex - 1];
                if (picked == only)
                {
                    return;
                }
                only = picked;
                _ = DispatcherQueue.TryEnqueue(DrawList);
            };
            Grid.SetColumn(who, 1);
            controls.Children.Add(who);
        }
        head.Children.Add(controls);
    }

    private void DrawList()
    {
        list.Items.Clear();
        var shown = digests
            .Where(digest => showing switch
            {
                "Unwritten" => !entries.ContainsKey(digest.Id),
                "Written" => entries.ContainsKey(digest.Id),
                _ => true,
            })
            .Where(digest => only is null || digest.DisplayName == only)
            .Where(digest => needle.Length == 0 || Cards.Matches(digest, entries.GetValueOrDefault(digest.Id), needle))
            .ToList();

        if (shown.Count == 0)
        {
            list.Items.Add(new ListViewItem { Content = NothingYet(), IsEnabled = false });
            selected = null;
            DrawDetail();
            return;
        }

        if (selected is null || shown.All(digest => digest.Id != selected))
        {
            selected = shown[0].Id;
        }
        var now = DateTimeOffset.UtcNow;
        var light = Widgets.IsLight(this);
        ListViewItem? chosen = null;
        foreach (var digest in shown)
        {
            var item = new ListViewItem { Content = Row(digest, now, light), Tag = digest.Id, Padding = new Thickness(8, 6, 8, 6) };
            if (digest.Id == selected)
            {
                chosen = item;
            }
            list.Items.Add(item);
        }
        list.Items.Add(new ListViewItem
        {
            Content = new TextBlock { Text = "Newest first, across every character.", Style = (Style)Application.Current.Resources["Caption"], Margin = new Thickness(0, 6, 0, 2) },
            IsEnabled = false,
        });
        // Selected after the items exist, and never during a measure pass.
        _ = DispatcherQueue.TryEnqueue(() =>
        {
            list.SelectedItem = chosen;
            DrawDetail();
        });
    }

    /// <summary>One row: the evening's title over who and what, with when and whether it is written on the right.</summary>
    private Grid Row(Digest digest, DateTimeOffset now, bool light)
    {
        var row = new Grid { ColumnSpacing = 12 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var entry = entries.GetValueOrDefault(digest.Id);
        var text = new StackPanel { Spacing = 2 };
        text.Children.Add(new TextBlock { Text = TitleOf(digest, entry), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextTrimming = TextTrimming.CharacterEllipsis });
        var who = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        who.Children.Add(new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 6, Height = 6, Fill = Widgets.ClassBrush(digest.Class, light), VerticalAlignment = VerticalAlignment.Center });
        who.Children.Add(new TextBlock { Text = $"{digest.DisplayName} · {Cards.MetaLine(digest, LocalOffset(digest))}", Style = (Style)Application.Current.Resources["Caption"], TextTrimming = TextTrimming.CharacterEllipsis });
        text.Children.Add(who);
        row.Children.Add(text);

        var right = new StackPanel { Spacing = 2, HorizontalAlignment = HorizontalAlignment.Right };
        right.Children.Add(new TextBlock { Text = Account.When(digest.StartedAt, now), Style = (Style)Application.Current.Resources["Caption"], HorizontalAlignment = HorizontalAlignment.Right });
        var marker = new TextBlock { Text = account.IsWriting(digest.Id) ? "Writing…" : entry is null ? "Unwritten" : "Written", FontSize = 12, HorizontalAlignment = HorizontalAlignment.Right };
        marker.Foreground = entry is null ? Widgets.Tertiary : Widgets.AccentBrush;
        right.Children.Add(marker);
        Grid.SetColumn(right, 1);
        row.Children.Add(right);
        return row;
    }

    private static string TitleOf(Digest digest, Entry? entry) => entry is { Title.Length: > 0 } ? entry.Title : digest.Headline();

    private static TimeSpan LocalOffset(Digest digest) => TimeZoneInfo.Local.GetUtcOffset(digest.StartedAt);

    /// <summary>
    /// What the page says before the addon has written anything. Two
    /// different nothings: nobody has installed the addon, or nobody has set
    /// the journal up. Saying "no entries" to the first would leave somebody
    /// waiting for a file that will never be written.
    /// </summary>
    private StackPanel NothingYet()
    {
        var stack = new StackPanel { Spacing = 10, Margin = new Thickness(4, 12, 4, 12) };
        if (digests.Count == 0)
        {
            stack.Children.Add(new TextBlock { Text = "No evenings recorded yet", Style = (Style)Application.Current.Resources["CardHeading"] });
            stack.Children.Add(Widgets.Text("The Chronicle records a session when you log out — where you went, what you turned in, what dropped. Install the collector addon, play, and log out once.", "Secondary"));
            if (!account.JournalReady)
            {
                var setup = Widgets.Accent("Set Up the Journal");
                setup.Click += async (_, _) => await Setup();
                stack.Children.Add(setup);
            }
        }
        else
        {
            stack.Children.Add(new TextBlock { Text = "No matching evenings", Style = (Style)Application.Current.Resources["CardHeading"] });
            stack.Children.Add(Widgets.Text("Nothing in the journal matches that.", "Secondary"));
        }
        return stack;
    }

    // -- the detail pane ------------------------------------------------------

    private void DrawDetail()
    {
        detail.Children.Clear();
        DrawHead();
        var digest = selected is { } id ? digests.FirstOrDefault(digest => digest.Id == id) : null;
        if (digest is null)
        {
            return;
        }
        var entry = entries.GetValueOrDefault(digest.Id);

        var titles = new StackPanel { Spacing = 4 };
        titles.Children.Add(ChronicleWidgets.SerifTitle(TitleOf(digest, entry)));
        titles.Children.Add(ChronicleWidgets.Meta($"{digest.DisplayName} · {Cards.MetaLine(digest, LocalOffset(digest))}"));
        detail.Children.Add(titles);

        if (entry is not null)
        {
            var quote = new Border { Style = (Style)Application.Current.Resources["Quote"], Child = ChronicleWidgets.Prose(entry.Body, italic: true) };
            detail.Children.Add(quote);
            var byline = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
            // Said on every entry, not once in an about box: the rest of
            // this pane is measurements from the person's own machine; this
            // paragraph is not.
            byline.Children.Add(new TextBlock { Text = string.Create(CultureInfo.InvariantCulture, $"Written by {entry.Model} on {entry.WrittenAt.ToLocalTime():d MMMM yyyy} ·"), Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Center });
            var again = new HyperlinkButton { Content = "Write it again", Padding = new Thickness(2, 0, 2, 0), IsEnabled = account.JournalReady && !account.IsWriting(digest.Id) };
            var id2 = digest.Id;
            again.Click += async (_, _) => await account.WriteEntry(id2);
            byline.Children.Add(again);
            detail.Children.Add(byline);
        }
        else if (account.IsWriting(digest.Id))
        {
            var waiting = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
            waiting.Children.Add(new ProgressRing { Width = 16, Height = 16, IsActive = true });
            waiting.Children.Add(new TextBlock { Text = "Writing…", Style = (Style)Application.Current.Resources["Caption"], VerticalAlignment = VerticalAlignment.Center });
            detail.Children.Add(waiting);
        }
        else if (!account.JournalReady)
        {
            detail.Children.Add(Widgets.Text("No entry yet. Set up the journal and a model writes one from what happened below.", "Caption"));
        }

        if (RouteChain(digest) is { } chain)
        {
            detail.Children.Add(chain);
        }
        // The same fact the Run page leads with, at the smaller size: it is
        // about this evening, and this is the evening it came from.
        if (digest.LongestFight > 0)
        {
            detail.Children.Add(Widgets.Stat("Longest fight", Cards.FightLength(digest.LongestFight)));
        }
        if (Ledger(digest) is { } books)
        {
            detail.Children.Add(books);
        }
        if (Overheard(digest) is { } said)
        {
            detail.Children.Add(said);
        }

        var facts = new StackPanel();
        facts.Children.Add(Widgets.CardHead("What happened", Cards.Tally(digest)));
        foreach (var (label, value) in Log(digest))
        {
            facts.Children.Add(ChronicleWidgets.Fact(label, value));
        }
        detail.Children.Add(Widgets.Card(facts));

        var pictures = digest.Pictures(shots);
        if (pictures.Count > 0)
        {
            detail.Children.Add(Gallery(pictures));
        }

        var reading = digest.FurtherReading();
        if (reading.Count > 0)
        {
            var links = new StackPanel();
            links.Children.Add(Widgets.CardHead("Further reading", "opens in your browser"));
            foreach (var link in reading)
            {
                links.Children.Add(new HyperlinkButton { Content = $"{link.Label} — {link.Sort.Label()}", NavigateUri = new Uri(link.Url), Padding = new Thickness(0, 2, 0, 2) });
            }
            detail.Children.Add(new Expander { Header = "Further reading", Content = links, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Stretch });
        }

        detail.Children.Add(Counters(digest));
        detailScroller.ChangeView(null, 0, null, disableAnimation: true);
    }

    /// <summary>The route as chips joined end to end. The stop the evening was spent at is the accent one, and carries how long.</summary>
    private static StackPanel? RouteChain(Digest digest)
    {
        if (digest.Route.Count == 0)
        {
            return null;
        }
        var longest = digest.Route.MaxBy(stop => stop.Stayed);
        var chain = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 0 };
        void Join()
        {
            if (chain.Children.Count > 0)
            {
                chain.Children.Add(new Border { Width = 18, Height = 1, VerticalAlignment = VerticalAlignment.Center, Background = Widgets.Tertiary, Opacity = 0.5 });
            }
        }
        foreach (var stop in digest.Route.Take(RouteShown))
        {
            Join();
            chain.Children.Add(stop == longest
                ? ChronicleWidgets.Chip($"{stop.Zone} · {Armory.Tally.Counters.Spent(stop.Stayed)}", accent: true)
                : ChronicleWidgets.Chip(stop.Zone));
        }
        if (digest.Route.Count > RouteShown)
        {
            Join();
            chain.Children.Add(ChronicleWidgets.Chip(string.Create(CultureInfo.InvariantCulture, $"{digest.Route.Count - RouteShown} more")));
        }
        return chain;
    }

    /// <summary>Where the gold went, as one bar. The ledger is the only set of books; the itemised moments are not a second one.</summary>
    private static Border? Ledger(Digest digest)
    {
        var (income, spending) = Cards.LedgerShares(digest.Income, digest.Spending);
        if (income.Count == 0 && spending.Count == 0)
        {
            return null;
        }
        var card = new StackPanel { Spacing = 8 };
        var heading = new Grid();
        heading.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        heading.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        heading.Children.Add(ChronicleWidgets.Meta("Where the gold went"));
        var net = ChronicleWidgets.Net(digest.Purse);
        Grid.SetColumn(net, 1);
        heading.Children.Add(net);
        card.Children.Add(heading);
        card.Children.Add(ChronicleWidgets.Ledger(income, spending));
        var legend = new TextBlock { Style = (Style)Application.Current.Resources["Caption"], TextWrapping = TextWrapping.Wrap };
        legend.Text = string.Join("   ", digest.Income.Take(2).Select(line => $"{Cards.Gold(line.Amount)} {line.Purpose.Label(true).ToLowerInvariant()}")
            .Concat(digest.Spending.Take(2).Select(line => $"{Cards.Gold(line.Amount)} {line.Purpose.Label(false).ToLowerInvariant()}")));
        card.Children.Add(legend);
        return Widgets.Card(card, dense: true);
    }

    /// <summary>One line the world said, pulled out of the evening. Said only, never Told: a shopkeeper's greeting keeps its own budget in the log.</summary>
    private static StackPanel? Overheard(Digest digest)
    {
        if (digest.Overheard.Count == 0)
        {
            return null;
        }
        var (who, line) = digest.Overheard[0];
        var block = new StackPanel { Spacing = 4 };
        block.Children.Add(ChronicleWidgets.Prose($"“{line}”", italic: true));
        var parts = new List<string>();
        if (who.Length > 0)
        {
            parts.Add(who);
        }
        if (Cards.SpentIn(digest) is { } zone)
        {
            parts.Add(zone);
        }
        var attribution = string.Join(", ", parts);
        if (digest.Overheard.Count > 1)
        {
            attribution += string.Create(CultureInfo.InvariantCulture, $" · {digest.Overheard.Count - 1} more");
        }
        if (attribution.Length > 0)
        {
            block.Children.Add(ChronicleWidgets.Meta(attribution));
        }
        return block;
    }

    /// <summary>The whole log, as labelled facts, in the order the GTK card lists them.</summary>
    private static List<(string Label, string Value)> Log(Digest digest)
    {
        var rows = new List<(string, string)>();
        void Add(string label, string value) => rows.Add((label, value));
        if (digest.Route.Count > 0)
        {
            Add("Route", string.Join(" → ", digest.Route.Select(stop => stop.Zone)));
        }
        foreach (var (name, summary) in digest.Campaigns)
        {
            Add("Campaign", summary is { Length: > 0 } ? $"{name} — {summary}" : name);
        }
        foreach (var key in digest.Keystones)
        {
            var upgrades = key.Upgrades == 0 ? "" : string.Create(CultureInfo.InvariantCulture, $" · key up {key.Upgrades}");
            Add(string.Create(CultureInfo.InvariantCulture, $"+{key.Level} {key.Dungeon}"), $"{(key.InTime ? "Timed" : "Over the timer")} in {Prose.Spell(TimeSpan.FromSeconds(key.Seconds))}{upgrades}");
        }
        if (digest.Instances.Count > 0 && digest.Keystones.Count == 0)
        {
            Add("Instances", string.Join(", ", digest.Instances.Select(instance => $"{instance.Name} ({instance.Kind})")));
        }
        if (digest.Scenarios.Count > 0)
        {
            Add("Scenarios", string.Join(", ", digest.Scenarios));
        }
        if (digest.WorldTiers.Count > 0)
        {
            Add("World tier", string.Join(", then ", digest.WorldTiers));
        }
        if (digest.Weather.Count > 0)
        {
            Add("Weather", string.Join(", then ", digest.Weather));
        }
        foreach (var quest in digest.Quests)
        {
            var story = quest.Story ?? quest.Premise;
            Add("Quest", string.IsNullOrEmpty(story) ? quest.Title : $"{quest.Title} — {story}");
        }
        if (digest.Felled.Count > 0)
        {
            Add("Defeated", string.Join(", ", digest.Felled));
        }
        if (digest.LostTo.Count > 0)
        {
            Add("Wiped on", string.Join(", ", digest.LostTo));
        }
        if (digest.Rares.Count > 0)
        {
            Add("Rares", string.Join(", ", digest.Rares));
        }
        foreach (var (_, name) in digest.Achievements)
        {
            Add("Achievement", name);
        }
        if (digest.Acquired.Count > 0)
        {
            Add("Collected", string.Join(", ", digest.Acquired.Select(got => $"{got.Name} ({got.Kind.Label()})")));
        }
        if (digest.Loot.Count > 0)
        {
            Add("Loot", string.Join(", ", digest.Loot.Select(loot => loot.Name)));
        }
        if (digest.Sales.Count > 0)
        {
            Add("Sold", string.Join(", ", digest.Sales.Select(sale => $"{sale.Subject} — {Prose.Money(sale.Money)}")));
        }
        if (digest.Deaths.Count > 0)
        {
            // What killed you is the half of a death worth reading.
            Add("Deaths", string.Join(", ", digest.Deaths.Select(death => death.To is { } to ? $"{death.Zone} — {to}" : death.Zone)));
        }
        if (digest.Risen.Count > 0)
        {
            Add("Standing", string.Join(", ", digest.Risen.Select(risen => $"{risen.Faction} → {Prose.Standing(risen.Rank)}")));
        }
        if (digest.Equipped.Count > 0)
        {
            Add("Upgraded", string.Join(", ", digest.Equipped.Take(5).Select(gear => gear.From is { } from
                ? string.Create(CultureInfo.InvariantCulture, $"{gear.Name} ({gear.ItemLevel}, off {from})")
                : string.Create(CultureInfo.InvariantCulture, $"{gear.Name} ({gear.ItemLevel})"))));
        }
        if (digest.Practised.Count > 0)
        {
            Add("Professions", string.Join(", ", digest.Practised.Select(skill => string.Create(CultureInfo.InvariantCulture, $"{skill.Profession} {skill.Skill}"))));
        }
        if (digest.Appearances.Count > 0)
        {
            Add("Appearances", string.Join(", ", digest.Appearances));
        }
        if (digest.Questgivers.Count > 0)
        {
            Add("Sent out by", string.Join(", ", digest.Questgivers.Select(giver => giver.Given == 1 ? giver.Who : string.Create(CultureInfo.InvariantCulture, $"{giver.Who} ×{giver.Given}"))));
        }
        if (digest.Learned.Count > 0)
        {
            Add("Learned", string.Join(", ", digest.Learned));
        }
        // A row per line rather than one joined row: these are sentences.
        foreach (var (who, line) in digest.Overheard.Take(OverheardShown))
        {
            Add(who.Length == 0 ? "Overheard" : who, line);
        }
        // Fewer than the overheard lines get, and never merged with them.
        foreach (var (who, line) in digest.Told.Take(ToldShown))
        {
            Add(who.Length == 0 ? "Said to you" : who, line);
        }
        if (digest.Cutscenes.Count > 0)
        {
            Add("Cutscenes", string.Join(", ", digest.Cutscenes.Select(scene => scene.Movie is { } movie ? string.Create(CultureInfo.InvariantCulture, $"{scene.Zone} (movie {movie})") : scene.Zone)));
        }
        if (digest.Expired.Count > 0)
        {
            Add("Came back unsold", string.Join(", ", digest.Expired));
        }
        if (digest.Companions.Count > 0)
        {
            Add("Alongside", string.Join(", ", digest.Companions));
        }
        if (digest.Crafted.Count > 0)
        {
            Add("Crafted", string.Join(", ", digest.Crafted.Select(made => made.Made == 1 ? made.Name : string.Create(CultureInfo.InvariantCulture, $"{made.Name} ×{made.Made}"))));
        }
        if (digest.Flights > 0)
        {
            Add("Flights", Prose.Plural(digest.Flights, "flight", "flights"));
        }
        if (digest.Travelled > 0)
        {
            Add("Travelled", Armory.Tally.Counters.Far(digest.Travelled));
        }
        if (digest.LongestFight > 0)
        {
            Add("Longest fight", Armory.Tally.Counters.Spent(digest.LongestFight));
        }
        Add("Purse", Prose.Purse(digest.Purse));
        // The books, biggest first. A net purse says an evening cost forty
        // gold; this says it earned three hundred questing and spent three
        // hundred and forty at the auction house, which is a different evening.
        if (digest.Income.Count > 0)
        {
            Add("Money in", string.Join(" · ", digest.Income.Select(line => $"{Prose.Money(line.Amount)} {line.Purpose.Label(true).ToLowerInvariant()}")));
        }
        if (digest.Spending.Count > 0)
        {
            Add("Money out", string.Join(" · ", digest.Spending.Select(line => $"{Prose.Money(line.Amount)} {line.Purpose.Label(false).ToLowerInvariant()}")));
        }
        return rows;
    }

    /// <summary>The evening's screenshots, loaded straight off disk by path: the player's own files, nothing to fetch and nothing to cache.</summary>
    private static ScrollViewer Gallery(List<Picture> pictures)
    {
        var strip = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        foreach (var picture in pictures)
        {
            var frame = new StackPanel { Spacing = 4, Width = 240 };
            frame.Children.Add(new Border
            {
                Width = 240,
                Height = 135,
                CornerRadius = new CornerRadius(4),
                Child = new Image { Source = new BitmapImage(new Uri(picture.Path)), Stretch = Microsoft.UI.Xaml.Media.Stretch.UniformToFill },
            });
            var caption = new TextBlock { Text = picture.Subject, Style = (Style)Application.Current.Resources["Caption"], TextTrimming = TextTrimming.CharacterEllipsis };
            ToolTipService.SetToolTip(caption, picture.Path);
            frame.Children.Add(caption);
            strip.Children.Add(frame);
        }
        return new ScrollViewer { Content = strip, HorizontalScrollMode = ScrollMode.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollMode = ScrollMode.Disabled };
    }

    /// <summary>
    /// The lifetime counters, drawn shut, in one group. Eight open lists is
    /// most of a screen and this is a journal: pushing tonight's evening
    /// below how many flasks somebody has made is the wrong page. Each row
    /// carries its headline so the shut state still answers.
    /// </summary>
    private Expander Counters(Digest digest)
    {
        var counted = tallies.GetValueOrDefault(digest.Character) ?? [];
        var body = new StackPanel { Spacing = 6 };
        body.Children.Add(Widgets.Text("Counters the game does not keep, since the addon was installed.", "Caption"));
        var any = false;
        foreach (var kind in Cards.RailOrder)
        {
            var rows = Armory.Tally.Counters.Of(counted, kind);
            if (rows.Count == 0)
            {
                continue;
            }
            if (any)
            {
                body.Children.Add(ChronicleWidgets.Hairline());
            }
            var first = rows[0];
            var block = new StackPanel { Spacing = 2 };
            block.Children.Add(new TextBlock { Text = kind.Title(), Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"] });
            var line = new Grid { ColumnSpacing = 8 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.Children.Add(new TextBlock { Text = first.Label, Style = (Style)Application.Current.Resources["Caption"], TextTrimming = TextTrimming.CharacterEllipsis });
            var count = ChronicleWidgets.Figure(Cards.CountedAs(kind, first.Count), accent: true);
            Grid.SetColumn(count, 1);
            line.Children.Add(count);
            block.Children.Add(line);
            // The rest of the list, where it costs no height.
            ToolTipService.SetToolTip(block, string.Join("\n", rows.Take(TallyShown).Select(row => $"{row.Label} — {Cards.Said(kind, row.Count)}")));
            body.Children.Add(block);
            any = true;
        }
        if (!any)
        {
            body.Children.Add(Widgets.Text("Nothing counted yet for this character.", "Secondary"));
        }
        var scope = digests.Where(d => d.Character == digest.Character).ToList();
        var minutes = scope.Sum(d => Math.Max((long)d.Duration.TotalMinutes, 0));
        body.Children.Add(ChronicleWidgets.Hairline());
        body.Children.Add(ChronicleWidgets.Meta(string.Create(CultureInfo.InvariantCulture, $"{Prose.Plural(scope.Count, "evening", "evenings")} · {Prose.Plural(minutes / 60, "hour", "hours")} · since {scope.Min(d => d.StartedAt).ToLocalTime():d MMMM yyyy}")));
        return new Expander
        {
            Header = $"{digest.DisplayName}, over time",
            Content = body,
            IsExpanded = false,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch,
        };
    }

    // -- actions ----------------------------------------------------------------

    private async Task Setup()
    {
        var dialog = new Dialogs.JournalDialog(account) { XamlRoot = XamlRoot };
        await dialog.ShowAsync();
    }

    /// <summary>
    /// Destructive, and the only thing here that cannot be undone by syncing
    /// again: the addon keeps its last forty sessions and this one may have
    /// fallen off the end. So it is confirmed rather than toasted with an
    /// undo that might not be able to deliver.
    /// </summary>
    private async Task ConfirmForget()
    {
        if (selected is not { } id || digests.FirstOrDefault(digest => digest.Id == id) is not { } digest)
        {
            return;
        }
        var dialog = new ContentDialog
        {
            Title = "Forget this evening?",
            Content = new TextBlock
            {
                Text = string.Create(CultureInfo.InvariantCulture, $"{digest.DisplayName}'s session on {digest.StartedAt.ToLocalTime():dddd d MMMM} will be removed from the journal, along with anything written about it. This cannot be undone."),
                TextWrapping = TextWrapping.Wrap,
            },
            PrimaryButtonText = "Forget",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = XamlRoot,
        };
        if (await dialog.ShowAsync() == ContentDialogResult.Primary)
        {
            selected = null;
            await account.ForgetSession(id);
        }
    }
}
