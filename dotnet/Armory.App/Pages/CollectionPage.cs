using System.Globalization;
using Armory.App.Dialogs;
using Armory.Client.Collections;
using Armory.Client.Shell;
using Armory.Collections;
using Armory.Roster;
using Armory.Tally;
using CommunityToolkit.Mvvm.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Media;

namespace Armory.App.Pages;

/// <summary>
/// One page serving mounts, pets, toys and decor. Only the noun, the tallies
/// and the search placeholder change. The grid is the scroll host; the
/// header and the three closest to earning ride above it and the rail's
/// cards below it, so a few thousand tiles are recycled rather than laid out
/// at once. An owned tile dims its art box and carries a dot; in a run,
/// owning something is the bad news. The words are worked out in
/// <see cref="CollectionCards"/>, which is where they are tested.
/// </summary>
public sealed partial class CollectionPage : Page, ISearchable
{
    private readonly Account account;
    private readonly Kind kind;
    private readonly string noun;
    private readonly GridView grid;
    private readonly StackPanel header = new() { Spacing = 16, Margin = new Thickness(0, 0, 0, 8) };
    private readonly StackPanel footer = new() { Spacing = 16, Margin = new Thickness(0, 24, 0, 0) };
    private string needle = "";
    private Showing showing = Showing.Missing;
    private Source? sourceFilter;
    private List<Collectible> catalogue = [];
    private HashSet<long> owned = [];
    private List<Armory.Tally.Tally> attempts = [];
    private List<Collectible> closest = [];
    private List<Collectible> shown = [];
    private Faction faction = Faction.Neutral;

    public CollectionPage(Account account, Kind kind)
    {
        this.account = account;
        this.kind = kind;
        noun = kind.Label().ToLowerInvariant();
        InitializeComponent();
        grid = new GridView
        {
            SelectionMode = ListViewSelectionMode.None,
            IsItemClickEnabled = true,
            ItemTemplate = (DataTemplate)Resources["TileTemplate"],
            Header = header,
            Footer = footer,
            Padding = (Thickness)Application.Current.Resources["PagePadding"],
        };
        grid.GroupStyle.Add((GroupStyle)Resources["SourceGroups"]);
        grid.ItemClick += (_, args) =>
        {
            if (args.ClickedItem is CollectionTile tile)
            {
                _ = CollectibleDialog.Show(XamlRoot, account, tile.Entry, tile.Owned);
            }
        };
        // Art is fetched for the tiles on screen and no others: the budget is
        // one request per entry and it has to be spent on what somebody is
        // looking at. The journal's sentence is the tooltip, because the cell
        // shows a name over one word and the sentence is the thing worth
        // reading.
        grid.ContainerContentChanging += (sender, args) =>
        {
            if (!args.InRecycleQueue && args.Item is CollectionTile tile)
            {
                ToolTipService.SetToolTip(args.ItemContainer, tile.Tooltip);
                _ = tile.EnsureArt(account);
            }
        };
        Content = grid;
        // The media pass asks the page which icons to spend its budget on:
        // the tiles being shown, in the order they are shown. One hook for
        // four pages, so each answers for its own kind and passes the rest on.
        var previous = account.ItemArtWanted;
        account.ItemArtWanted = (asked, budget) => asked == kind ? ArtWanted(budget) : previous?.Invoke(asked, budget) ?? [];
        account.Changed += Reload;
        Reload();
    }

    /// <summary>
    /// The item ids of toys or decor with no icon yet, up to the budget: what
    /// is shown first, in the order it is shown, and then everything held.
    /// </summary>
    private List<long> ArtWanted(int budget) => kind is Kind.Toy or Kind.Decor
        ? CollectionCards.ArtWanted(shown, catalogue, entry => account.ArtUrl(entry) is not null, budget)
        : [];

    public string SearchPlaceholder => $"Search {noun}";

    public void Search(string text)
    {
        needle = text.Trim();
        Redraw();
    }

    private async void Reload()
    {
        var read = await account.Store.On(store => (
            Held: store.CollectiblesHeld(kind).Match(held => held, _ => ([], [])),
            Tallies: store.TalliesHeld().Match(held => held, _ => [])));
        catalogue = read.Held.Catalogue;
        owned = read.Held.Owned;
        // Every pull this account has ever made, merged across characters: a
        // mount is account-wide, so an alt's raiding is rolls at it too. This
        // is the only place "58 TRIES" exists; Blizzard forgets an attempt the
        // moment the encounter ends.
        attempts = Counters.AccountAttempts(read.Tallies);
        // Whichever side the cohort plays. Faction-locked entries the account
        // can never have are not a gap in its collection.
        faction = account.Cohort.MembersOf(account.Roster).FirstOrDefault()?.Faction
            ?? account.Roster.Characters.FirstOrDefault()?.Faction
            ?? Faction.Neutral;
        closest = CollectionCards.PickClosest(catalogue, owned, faction);
        Redraw();
    }

    private void Redraw()
    {
        header.Children.Clear();
        footer.Children.Clear();
        var standing = CollectionCards.Counts(catalogue, owned, faction);
        var title = kind.Label();

        var sources = new ComboBox { MinWidth = 160 };
        sources.Items.Add("Every source");
        foreach (var source in SourceGroups.Groups)
        {
            sources.Items.Add(SourceGroups.RailLabel(source));
        }
        sources.SelectedIndex = sourceFilter is { } chosen ? SourceGroups.Rank(chosen) + 1 : 0;
        sources.SelectionChanged += (_, _) =>
        {
            Source? picked = sources.SelectedIndex <= 0 ? null : SourceGroups.Groups[sources.SelectedIndex - 1];
            ChooseSource(picked);
        };
        header.Children.Add(Widgets.Header(
            title,
            string.Create(CultureInfo.InvariantCulture, $"{standing.Collected:N0} of {standing.Countable:N0} collected"),
            sources));

        var selector = new SelectorBar();
        foreach (var view in CollectionCards.Toggle)
        {
            selector.Items.Add(new SelectorBarItem { Text = view.Label(), IsSelected = view == showing });
        }
        selector.SelectionChanged += (sender, _) =>
        {
            var picked = CollectionCards.Toggle.FirstOrDefault(view => view.Label() == sender.SelectedItem?.Text, Showing.Missing);
            if (picked == showing)
            {
                return;
            }
            showing = picked;
            DispatcherQueue.TryEnqueue(Redraw);
        };
        header.Children.Add(selector);

        // A rail saying "0 of 0" over a page that has never synced reads as a
        // broken collection rather than an empty one, and the three cards
        // above an unsynced grid have nothing to be closest to.
        if (catalogue.Count == 0)
        {
            header.Children.Add(Widgets.Standing("Nothing synced yet", kind == Kind.Decor
                ? "Sync to fetch the housing catalogue and what your account has of it, or log out once with the collector addon installed: it reads the catalogue the in-game window does, with the source text and the counts the web API has no field for."
                : "Sync to fetch what your account has collected and what exists to collect. Logging out once with the collector addon installed brings the whole catalogue in one go, with the artwork and the in-game source text the web API has no field for."));
            grid.ItemsSource = null;
            return;
        }

        if (closest.Count > 0)
        {
            header.Children.Add(ClosestBlock());
        }

        // The toggle and the search decide what is shown; the source is the
        // grouping, and choosing one is what opens its own page.
        shown = CollectionCards.Shown(catalogue, owned, faction, showing, needle);
        var held = shown.GroupBy(entry => entry.Source).ToDictionary(group => group.Key, group => group.Count());
        footer.Children.Add(StandingCard(standing));
        footer.Children.Add(WhereTheyComeFrom(held));

        var page = sourceFilter is { } only ? shown.Where(entry => entry.Source == only).ToList() : shown;
        if (page.Count == 0)
        {
            var empty = new StackPanel { Spacing = 12, HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(0, 40, 0, 40) };
            empty.Children.Add(new TextBlock { Text = "No matches", Style = (Style)Application.Current.Resources["Subtitle"], HorizontalAlignment = HorizontalAlignment.Center });
            empty.Children.Add(Widgets.Text("Nothing here fits the search and filters.", "Secondary"));
            var clear = Widgets.Standard("Clear Filters");
            clear.HorizontalAlignment = HorizontalAlignment.Center;
            clear.Click += (_, _) =>
            {
                needle = "";
                sourceFilter = null;
                DispatcherQueue.TryEnqueue(Redraw);
            };
            empty.Children.Add(clear);
            header.Children.Add(Widgets.Card(empty));
            grid.ItemsSource = null;
            return;
        }

        // Grouped by source in the page's reading order, alphabetical within
        // a group: the catalogue's own order is by id, which is the newest
        // things in the game first. Shown is already in that order.
        var counted = showing.Counted();
        var groups = page
            .GroupBy(entry => entry.Source)
            .Select(group =>
            {
                var tiles = new TileGroup(string.Create(CultureInfo.InvariantCulture, $"{SourceGroups.Heading(group.Key)} · {group.Count():N0} {counted}"));
                tiles.AddRange(group.Select(Tile));
                return tiles;
            })
            .ToList();
        grid.ItemsSource = new CollectionViewSource { IsSourceGrouped = true, Source = groups }.View;
    }

    private CollectionTile Tile(Collectible entry)
    {
        var had = owned.Contains(entry.Id);
        // Owned is only worth marking where both kinds are on screen. Under
        // "Collected" every entry is owned, so dimming the lot says nothing
        // and makes a page of six hundred harder to read than it needs to be.
        var spent = had && showing != Showing.Collected;
        // "Unobtainable" is only ever said of something not had. It means
        // "you cannot get this", which is not a thing to tell somebody about
        // a mount already in their collection.
        var note = spent ? "already owned" : !had && !entry.ObtainableBy(faction) ? "unobtainable" : null;
        return new CollectionTile(entry, had, spent, note, CollectionCards.Tooltip(entry), account.ArtUrl(entry));
    }

    private void ChooseSource(Source? picked)
    {
        if (picked == sourceFilter)
        {
            return;
        }
        sourceFilter = picked;
        DispatcherQueue.TryEnqueue(Redraw);
    }

    /// <summary>
    /// The three nearest to earning, above the catalogue: they are the answer
    /// to "what should I do tonight", and the day the lockout turns over is
    /// the clock a collector plans that around.
    /// </summary>
    private StackPanel ClosestBlock()
    {
        var block = new StackPanel { Spacing = 8 };
        block.Children.Add(new TextBlock
        {
            Text = $"CLOSEST TO EARNING — LOCKOUT RESETS {CollectionCards.ResetDay(account.Settings.Region)}",
            Style = (Style)Application.Current.Resources["Caption"],
        });
        var row = new Grid { ColumnSpacing = 16 };
        for (var i = 0; i < closest.Count; i++)
        {
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var card = ClosestCard(closest[i]);
            Grid.SetColumn(card, i);
            row.Children.Add(card);
        }
        block.Children.Add(row);
        return block;
    }

    /// <summary>
    /// One of the three, and what can honestly be said about earning it. Two
    /// different gold lines, and neither is a drop rate: the first says
    /// chance is not in the way at all; the second is Rarity's estimate over
    /// how many times this account has already fought the thing that drops
    /// it, and the tooltip says which is which.
    /// </summary>
    private Border ClosestCard(Collectible entry)
    {
        var fought = Counters.AttemptsAt(entry.Description, attempts);
        var odds = account.Chances.OneIn(entry);
        var claim = CollectionCards.Claim(entry.Source, odds, fought);

        var stack = new StackPanel { Spacing = 2 };
        stack.Children.Add(new TextBlock { Text = entry.Name, Style = (Style)Application.Current.Resources["BodyStrongTextBlockStyle"], TextTrimming = TextTrimming.CharacterEllipsis });
        stack.Children.Add(new TextBlock { Text = CollectionCards.Whence(entry), Style = (Style)Application.Current.Resources["Caption"], TextTrimming = TextTrimming.CharacterEllipsis });
        if (claim is not null)
        {
            // Gold only where there is something gold to say.
            var line = new TextBlock
            {
                Text = claim,
                Style = (Style)Application.Current.Resources["Figure"],
                FontFamily = new FontFamily("Cascadia Mono, Consolas"),
                Foreground = Widgets.AccentBrush,
                Margin = new Thickness(0, 6, 0, 0),
                TextTrimming = TextTrimming.CharacterEllipsis,
            };
            if (CollectionCards.ClaimTooltip(odds, fought) is { } meaning)
            {
                ToolTipService.SetToolTip(line, meaning);
            }
            stack.Children.Add(line);
        }

        var card = Widgets.Card(stack, dense: true);
        card.PointerPressed += (_, _) => _ = CollectibleDialog.Show(XamlRoot, account, entry, false);
        return card;
    }

    /// <summary>
    /// The standing: what is had, out of what this account could ever hold.
    /// The line underneath says what was left out, so the difference from the
    /// catalogue's own total is visible rather than quietly applied.
    /// </summary>
    private static Border StandingCard(Standing standing)
    {
        var stack = new StackPanel { Spacing = 8 };
        var figures = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        figures.Children.Add(new TextBlock
        {
            Text = standing.Collected.ToString("N0", CultureInfo.InvariantCulture),
            Style = (Style)Application.Current.Resources["LargeFigure"],
            FontFamily = new FontFamily("Cascadia Mono, Consolas"),
        });
        figures.Children.Add(new TextBlock
        {
            Text = string.Create(CultureInfo.InvariantCulture, $"of {standing.Countable:N0}"),
            Style = (Style)Application.Current.Resources["Caption"],
            VerticalAlignment = VerticalAlignment.Bottom,
            Margin = new Thickness(0, 0, 0, 6),
        });
        stack.Children.Add(figures);
        stack.Children.Add(Widgets.Bar(standing.Fraction));
        if (standing.Unobtainable > 0)
        {
            stack.Children.Add(Widgets.Text(string.Create(CultureInfo.InvariantCulture, $"{standing.Unobtainable:N0} more can never be taken again by this account, and are left out of the count."), "Caption"));
        }
        return Widgets.Card(stack);
    }

    /// <summary>
    /// Where they come from, which is also the filter: choosing one is what
    /// opens its own page in the main column. The caveat is not the design's:
    /// the mock credits addons for the odds and the sources, and the card
    /// says what is actually behind every figure on the page instead.
    /// </summary>
    private Border WhereTheyComeFrom(Dictionary<Source, int> held)
    {
        var stack = new StackPanel { Spacing = 4 };
        stack.Children.Add(Widgets.CardHead("Where they come from"));
        foreach (var source in SourceGroups.Groups)
        {
            if (!held.TryGetValue(source, out var count) || count == 0)
            {
                continue;
            }
            var active = sourceFilter == source;
            var line = new Grid();
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.Children.Add(new TextBlock { Text = SourceGroups.RailLabel(source), FontWeight = active ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal });
            var figure = new TextBlock
            {
                Text = count.ToString("N0", CultureInfo.InvariantCulture),
                FontFamily = new FontFamily("Cascadia Mono, Consolas"),
                Foreground = active ? Widgets.AccentBrush : Widgets.Tertiary,
            };
            Grid.SetColumn(figure, 1);
            line.Children.Add(figure);

            var button = new Button
            {
                Content = line,
                Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                BorderThickness = new Thickness(0),
                Padding = new Thickness(8, 6, 8, 6),
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Stretch,
            };
            var chosen = source;
            button.Click += (_, _) => ChooseSource(sourceFilter == chosen ? null : chosen);
            stack.Children.Add(button);
        }
        stack.Children.Add(new Border { Style = (Style)Application.Current.Resources["RowDivider"], Margin = new Thickness(0, 8, 0, 0) });
        stack.Children.Add(Widgets.Text("There are no drop rates here. A source is Blizzard's one word and whatever sentence the collector addon recorded — the odds would need AllTheThings, which is not parsed. Opening an entry links out to Wowhead for the rest.", "Caption"));
        return Widgets.Card(stack);
    }
}

/// <summary>One source's tiles, for the group header.</summary>
public sealed class TileGroup(string label) : List<CollectionTile>
{
    public string Label { get; } = label;
}

/// <summary>One tile's data, for the template. The picture arrives after the tile is on screen.</summary>
public sealed class CollectionTile(Collectible entry, bool owned, bool spent, string? note, string tooltip, string? artUrl) : ObservableObject
{
    private ImageSource? art;
    private bool asked;

    public Collectible Entry { get; } = entry;

    public bool Owned { get; } = owned;

    public string Tooltip { get; } = tooltip;

    public string? ArtUrl { get; } = artUrl;

    /// <summary>An id with no name is an ownership sync that outran the catalogue. Saying so beats drawing a blank cell.</summary>
    public string Name => Entry.Name.Length > 0 ? Entry.Name : "Not named yet";

    /// <summary>The line under the name: "already owned" or "unobtainable" where either is true, else where it comes from.</summary>
    public string Source => note ?? Entry.Description?.Split('\n')[0] ?? Entry.Source.Label();

    public double ArtOpacity => spent ? 0.45 : 1.0;

    public Visibility Dot => spent ? Visibility.Visible : Visibility.Collapsed;

    public ImageSource? Art
    {
        get => art;
        private set => SetProperty(ref art, value);
    }

    /// <summary>Fetch the picture once, when the tile is first shown.</summary>
    public async Task EnsureArt(Account account)
    {
        if (asked || ArtUrl is null)
        {
            return;
        }
        asked = true;
        Art = await ArtLoader.Decode(account.Art, ArtUrl, 86);
    }
}
